//! The Android host seals vault records in Android Keystore. JNI stays entirely
//! inside Rust; neither keys nor vault entry points are exposed to the WebView.
use super::{unavailable, OsVault};
use jni::{
    errors::{Error as JniError, Result as JniResult},
    objects::{JByteArray, JClass, JObject, JValue},
    JNIEnv, JavaVM,
};
use nekopak_domain::ClientError;
use zeroize::Zeroizing;

const MAX_SECRET_BYTES: usize = 65_536;

fn with_env<T>(operation: impl FnOnce(&mut JNIEnv<'_>) -> JniResult<T>) -> Result<T, ClientError> {
    // Tauri/Wry initializes ndk_context before the Rust mobile entry point and
    // retains the VM and Context for the native application's lifetime.
    let context = ndk_context::android_context();
    // SAFETY: ndk_context's VM is supplied and owned by the Android runtime.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.map_err(|_| unavailable())?;
    let mut env = vm.attach_current_thread().map_err(|_| unavailable())?;
    // Tokio workers are long-lived. Every call releases its JNI locals instead
    // of keeping an unbounded set of objects alive on attached threads.
    let result = env.with_local_frame(24, operation);
    let failed = env.exception_check().unwrap_or(true);
    if failed {
        // Never describe/log Java exceptions: OS errors could include data or
        // paths. Also do not leave a pending exception on a reusable worker.
        let _ = env.exception_clear();
        return Err(unavailable());
    }
    result.map_err(|_| unavailable())
}

fn bridge<'local>(env: &mut JNIEnv<'local>) -> JniResult<(JObject<'local>, JClass<'local>)> {
    let context = ndk_context::android_context();
    // SAFETY: ndk_context owns this reference; JObject does not delete it. Make
    // a new local reference, released by with_env's frame, before using it.
    let raw = unsafe { JObject::from_raw(context.context().cast()) };
    let context = env.new_local_ref(&raw)?;
    let loader = env
        .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let name = env.new_string("io.nekopak.desktop.SessionVault")?;
    // find_class on a Rust worker uses the boot classloader. Load our retained
    // application class with the actual Android Context's classloader instead.
    let class = env
        .call_method(
            loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )?
        .l()?;
    Ok((context, JClass::from(class)))
}

fn clear_pending<T>(env: &mut JNIEnv<'_>, result: JniResult<T>) -> JniResult<T> {
    if env.exception_check()? {
        env.exception_clear()?;
        Err(JniError::JavaException)
    } else {
        result
    }
}

impl OsVault {
    pub(super) fn platform_load(&self) -> Result<Option<Zeroizing<String>>, ClientError> {
        let bytes = with_env(|env| {
            let (context, class) = bridge(env)?;
            let target = env.new_string(&self.target)?;
            let value = env
                .call_static_method(
                    class,
                    "load",
                    "(Landroid/content/Context;Ljava/lang/String;)[B",
                    &[JValue::Object(&context), JValue::Object(&target)],
                )?
                .l()?;
            if value.is_null() {
                return Ok(None);
            }
            let array = JByteArray::from(value);
            let length = env.get_array_length(&array)? as usize;
            if length == 0 || length > MAX_SECRET_BYTES {
                return Err(JniError::NullPtr("invalid vault record"));
            }
            let copied = env.convert_byte_array(&array);
            let copied = clear_pending(env, copied).map(Zeroizing::new);
            let wipe = env.set_byte_array_region(&array, 0, &vec![0; length]);
            wipe?;
            copied.map(Some)
        })?;
        bytes
            .map(|bytes| {
                std::str::from_utf8(&bytes)
                    .map(|text| Zeroizing::new(text.to_owned()))
                    .map_err(|_| unavailable())
            })
            .transpose()
    }

    pub(super) fn platform_save(&self, value: &str) -> Result<(), ClientError> {
        if value.is_empty() || value.len() > MAX_SECRET_BYTES {
            return Err(unavailable());
        }
        with_env(|env| {
            let (context, class) = bridge(env)?;
            let target = env.new_string(&self.target)?;
            let bytes = env.byte_array_from_slice(value.as_bytes())?;
            let result = env.call_static_method(
                class,
                "save",
                "(Landroid/content/Context;Ljava/lang/String;[B)V",
                &[
                    JValue::Object(&context),
                    JValue::Object(&target),
                    JValue::Object(&bytes),
                ],
            );
            let result = clear_pending(env, result);
            // Kotlin clears this buffer in finally as well. Clear on the Rust
            // side even if the method itself could not be entered.
            env.set_byte_array_region(&bytes, 0, &vec![0; value.len()])?;
            result.map(|_| ())
        })
    }

    pub(super) fn platform_delete(&self) -> Result<(), ClientError> {
        with_env(|env| {
            let (context, class) = bridge(env)?;
            let target = env.new_string(&self.target)?;
            env.call_static_method(
                class,
                "delete",
                "(Landroid/content/Context;Ljava/lang/String;)V",
                &[JValue::Object(&context), JValue::Object(&target)],
            )?;
            Ok(())
        })
    }
}
