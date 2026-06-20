#![forbid(unsafe_code)]

use watchtower_rs::types::ImageID;

fn short_id(id: &str) -> String {
    // Proxy to the types implementation, relocated due to package dependency resolution.
    ImageID::from(id).short_id()
}

#[test]
fn short_id_matches_legacy_container_utils_contract() {
    assert_eq!(
        short_id("sha256:0123456789abcd00000000001111111111222222222233333333334444444444"),
        "0123456789ab"
    );
    assert_eq!(
        short_id("0123456789abcd00000000001111111111222222222233333333334444444444"),
        "0123456789ab"
    );
    assert_eq!(short_id("0123456789ab"), "0123456789ab");
    assert_eq!(short_id("sha256:0123456789ab"), "0123456789ab");
    assert_eq!(short_id("md5:0123456789ab"), "md5:0123456789ab");
    assert_eq!(short_id("md5:0123456789abcdefg"), "md5:0123456789ab");
    assert_eq!(short_id("md5:01"), "md5:01");
}
