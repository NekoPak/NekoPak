//! Explicit recovery metadata and GCID creation. No automatic cleanup or write replay.
use super::*;

impl NativeProvider {
    pub(super) async fn native_recovery_metadata(
        &self,
        id: &str,
        scope: &str,
    ) -> Result<RecoveryMetadata, ClientError> {
        wire::validate_id(id)?;
        let epoch = self.checked_scope(scope).await?;
        let (value, _) = self
            .drive_request_at(
                Method::GET,
                &format!("{}/drive/v1/files/{id}", self.http.drive_origin),
                None,
                Some(epoch),
            )
            .await?;
        if value.get("phase").and_then(Value::as_str) != Some("PHASE_TYPE_COMPLETE") {
            return Err(wire::invalid());
        }
        let entry = wire::entry_in(&value, true)?;
        if entry.id != id {
            return Err(wire::invalid());
        }
        let trashed = value
            .get("trashed")
            .and_then(Value::as_bool)
            .ok_or_else(wire::invalid)?;
        Ok(RecoveryMetadata { entry, trashed })
    }
    pub(super) async fn native_create_from_content(
        &self,
        entry: &RemoteEntry,
        scope: &str,
    ) -> Result<ContentCreateReceipt, ClientError> {
        let hash = entry
            .hash
            .as_ref()
            .filter(|h| {
                h.algorithm == HashAlgorithm::Gcid
                    && h.value.len() == 40
                    && h.value.bytes().all(|b| b.is_ascii_hexdigit())
            })
            .ok_or_else(wire::invalid)?;
        let parent = entry.parent_id.as_deref().ok_or_else(wire::invalid)?;
        wire::validate_id(parent)?;
        if entry.kind != EntryKind::File
            || entry.name.is_empty()
            || entry.name.len() > 1024
            || entry.name.contains(['/', '\\'])
            || entry.name.chars().any(char::is_control)
        {
            return Err(wire::invalid());
        }
        let epoch = self.checked_scope(scope).await?;
        let body = json!({"kind":"drive#file","name":entry.name,"size":entry.size_bytes.0.to_string(),"hash":hash.value,"upload_type":"UPLOAD_TYPE_RESUMABLE","parent_id":if parent=="root"{""}else{parent},"objProvider":{"provider":"UPLOAD_TYPE_UNKNOWN"}});
        let (value, _) = self
            .drive_request_at(
                Method::POST,
                &format!("{}/drive/v1/files", self.http.drive_origin),
                Some(&body),
                Some(epoch),
            )
            .await
            .map_err(cloud::submitted_error)?;
        let file = value
            .get("file")
            .ok_or_else(wire::invalid)
            .map_err(cloud::submitted_error)?;
        let file_id = wire::text(file, "id", 256).map_err(cloud::submitted_error)?;
        wire::validate_id(&file_id).map_err(cloud::submitted_error)?;
        // Do not stat here: a post-accept failure must not discard the newly
        // created ID. Core persists this receipt before independent validation.
        let completed = file.get("phase").and_then(Value::as_str) == Some("PHASE_TYPE_COMPLETE");
        Ok(ContentCreateReceipt { file_id, completed })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::tests::Server;
    #[tokio::test]
    async fn recovery_requires_an_explicit_completed_phase() {
        let server=Server::new(|request|match request.path.split('?').next().unwrap(){
            "/drive/v1/files/missing-phase"=>(200,json!({"id":"missing-phase","name":"content.bin","kind":"drive#file","parent_id":"root","size":"7","hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","trashed":false})),
            "/drive/v1/files/pending"=>(200,json!({"id":"pending","name":"content.bin","kind":"drive#file","parent_id":"root","size":"7","hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","trashed":false,"phase":"PHASE_TYPE_PENDING"})),
            _=>(404,json!({"error":"file_not_found"})),
        }).await;
        let p = server.provider().await;
        let scope = p.account_scope().await.unwrap();
        for id in ["missing-phase", "pending"] {
            assert_eq!(
                p.recovery_metadata(id, &scope).await.unwrap_err().code,
                ErrorCode::IntegrityFailed
            );
        }
    }
    #[tokio::test]
    async fn recovery_reads_trash_state_and_creation_keeps_pending_node_without_delete() {
        let server=Server::new(|request|match (request.method.as_str(),request.path.split('?').next().unwrap()){
            ("GET","/drive/v1/files/original")=>(200,json!({"id":"original","name":"content.bin","kind":"drive#file","parent_id":"parent","size":"7","hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","trashed":true,"phase":"PHASE_TYPE_COMPLETE"})),
            ("POST","/drive/v1/files")=>{assert_eq!(request.body["upload_type"],"UPLOAD_TYPE_RESUMABLE");assert_eq!(request.body["hash"],"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");assert_eq!(request.body["size"],"7");assert_eq!(request.body["parent_id"],"parent");(200,json!({"file":{"id":"pending-new","phase":"PHASE_TYPE_PENDING"},"resumable":{"params":{"access_key_secret":"must-not-escape"}}}))},
            _=>(404,json!({"error":"file_not_found"})),
        }).await;
        let p = server.provider().await;
        let scope = p.account_scope().await.unwrap();
        assert!(p.stat("original").await.is_err());
        let metadata = p.recovery_metadata("original", &scope).await.unwrap();
        assert!(metadata.trashed);
        let receipt = p
            .create_from_content(&metadata.entry, &scope)
            .await
            .unwrap();
        assert!(!receipt.completed);
        assert_eq!(receipt.file_id, "pending-new");
        assert!(!serde_json::to_string(&receipt)
            .unwrap()
            .contains("must-not-escape"));
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.method == "POST" && r.path == "/drive/v1/files")
                .count(),
            1
        );
        assert!(!server
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.method == "DELETE" || r.path.contains("batchDelete")));
        let mut wrong = metadata.entry;
        wrong.hash.as_mut().unwrap().algorithm = HashAlgorithm::Sha1;
        assert!(p.create_from_content(&wrong, &scope).await.is_err());
    }
}
