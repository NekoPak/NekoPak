//! Public metadata from the browser-delivered PikPak Web login client (2026-11-08).
//! No SDK implementation, client secret, account credential or Android identity is copied.
//! Source hashes and exact request semantics: docs/NATIVE_LOGIN_RESEARCH.md.
//! https://mypikpak.com/drive/3877.4df17470.js (client ID)
//! https://mypikpak.com/drive/7991.9cc3c273.js (SDK/protocol headers)
//! The login initialization source for app version is recorded in the research note.

pub const WEB_CLIENT_ID: &str = "YUMx5nI8ZU8Ap8pm";
pub const LOGIN_CLIENT_VERSION: &str = "1.0.0";
pub const SDK_VERSION: &str = "8.1.4";
pub const PROTOCOL_VERSION: &str = "301";

// Public Web configuration, not an account secret or a human-verification answer.
// Exact asset digest, independent test vectors and semantics: DRIVE_PROTOCOL_RESEARCH.md.
const DRIVE_VERSION: &str = "2.0.0";
const DRIVE_PACKAGE: &str = "mypikpak.com";
const DRIVE_TIMESTAMP: &str = "1793961611384";
const DRIVE_SALTS: [&str; 15] = [
    "C9qPpZLN8ucRTaTiUMWYS9cQvWOE",
    "+r6CQVxjzJV6LCV",
    "F",
    "pFJRC",
    "9WXYIDGrwTCz2OiVlgZa90qpECPD6olt",
    "/750aCr4lm/Sly/c",
    "RB+DT/gZCrbV",
    "",
    "CyLsf7hdkIRxRm215hl",
    "7xHvLi2tOYP0Y92b",
    "ZGTXXxu8E/MIWaEDB+Sm/",
    "1UI3",
    "E7fP5Pfijd+7K+t6Tg/NhuLq0eEUVChpJSkrKxpO",
    "ihtqpG6FMt65+Xk+tWUH2",
    "NhXXU9rg4XXdzo7u5o",
];

pub fn drive_metadata(client_id: &str, device_id: &str, user_id: &str) -> serde_json::Value {
    use md5::{Digest, Md5};
    let mut digest =
        format!("{client_id}{DRIVE_VERSION}{DRIVE_PACKAGE}{device_id}{DRIVE_TIMESTAMP}");
    for salt in DRIVE_SALTS {
        digest = format!("{:x}", Md5::digest(format!("{digest}{salt}")));
    }
    serde_json::json!({"captcha_sign":format!("1.{digest}"),"client_version":DRIVE_VERSION,
        "package_name":DRIVE_PACKAGE,"user_id":user_id,"timestamp":DRIVE_TIMESTAMP})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_drive_configuration_matches_independent_vectors() {
        assert_eq!(
            drive_metadata(
                WEB_CLIENT_ID,
                "00000000000000000000000000000000",
                "synthetic"
            )["captcha_sign"],
            "1.d9941f4c807b846f52c4ff39f7becc3a"
        );
        assert_eq!(
            drive_metadata(
                WEB_CLIENT_ID,
                "7e60450667fa41f4838a1bac6ec204da",
                "synthetic"
            )["captcha_sign"],
            "1.4301f15b165ad1427c894397d4682e4e"
        );
    }
}
