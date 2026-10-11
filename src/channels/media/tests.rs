use super::*;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};
#[test]
fn aes_roundtrips_both_key_encodings_and_rejects_bad_padding() -> Result<()> {
    let key = [0x21; 16];
    let hex = key.iter().map(|b| format!("{b:02x}")).collect::<String>();
    for size in [1, 15, 16, 17, 32, 513] {
        let bytes = vec![0x87; size];
        let encrypted = encrypt(bytes.clone(), &key);
        assert_eq!(encrypted.len(), (size / 16 + 1) * 16);
        assert_eq!(
            decrypt(encrypted.clone(), &STANDARD.encode(key), false)?,
            bytes
        );
        assert_eq!(
            decrypt(encrypted.clone(), &STANDARD.encode(hex.as_bytes()), false)?,
            bytes
        );
        assert_eq!(decrypt(encrypted, &hex, true)?, bytes);
    }
    assert!(decrypt(vec![0; 16], "invalid", false).is_err());
    assert!(decrypt(vec![0; 15], &hex, true).is_err());
    assert!(decrypt(vec![0; 16], &"界".repeat(10), true).is_err());
    let mut wrong = encrypt(vec![0; 16], &key);
    wrong.truncate(16);
    assert!(decrypt(wrong, &hex, true).is_err());
    Ok(())
}
#[test]
fn media_metadata_stays_route_scoped_and_all_four_types_parse() -> Result<()> {
    let qq = qq(&json!({"attachments":[
        {"filename":"../../secret.png","content_type":"image/png","url":"//gchat.qpic.cn/photo"},
        {"filename":"voice.silk","content_type":"application/octet-stream","url":"https://multimedia.nt.qq.com/voice","asr_refer_text":"platform text"},
        {"filename":"clip.mp4","content_type":"video/mp4","url":"https://multimedia.nt.qq.com/video"},
        {"filename":"log.zip","url":"https://multimedia.nt.qq.com/file"}]}));
    assert_eq!(
        qq.iter().map(|a| a.kind.as_str()).collect::<Vec<_>>(),
        ["image", "audio", "video", "file"]
    );
    assert_eq!(qq[0].name, "secret.png");
    assert!(qq[0].url.starts_with("https://"));
    let wx = weixin(&json!({"item_list":[
        {"type":2,"image_item":{"media":{"encrypt_query_param":"token+&","aes_key":"key"},"aeskey":"hex"}},
        {"type":3,"voice_item":{"media":{"full_url":"https://novac2c.cdn.weixin.qq.com/voice"},"text":"platform words"}},
        {"type":4,"file_item":{"file_name":"report.txt","media":{"full_url":"https://novac2c.cdn.weixin.qq.com/file"}}},
        {"type":5,"video_item":{"media":{"full_url":"https://novac2c.cdn.weixin.qq.com/video"}}}]}));
    assert_eq!(wx.len(), 4);
    assert!(wx[0].raw_hex_key);
    assert_eq!(
        url::Url::parse(&wx[0].url)?
            .query_pairs()
            .next()
            .map(|(_, v)| v.to_string()),
        Some("token+&".into())
    );
    assert_eq!(wx[1].transcript.as_deref(), Some("platform words"));
    Ok(())
}
#[test]
fn only_platform_cdn_urls_are_accepted() {
    for bad in [
        "http://127.0.0.1/private",
        "http://169.254.169.254/latest",
        "file:///etc/passwd",
        "https://evilqq.com/a",
        "https://multimedia.nt.qq.com.evil.test/a",
        "https://user:secret@gchat.qpic.cn/a",
        "https://multimedia.nt.qq.com:8443/a",
    ] {
        assert!(validate_url("qq", bad).is_err(), "{bad}");
    }
    assert!(validate_url("qq", "https://multimedia.nt.qq.com/download?token=abc").is_ok());
    assert!(validate_url("weixin", "https://novac2c.cdn.weixin.qq.com/c2c/download").is_ok());
    assert!(validate_url("weixin", "https://arbitrary.weixin.qq.com/a").is_err());
}
#[tokio::test]
async fn downloads_enforce_limits_on_oversized_bodies() -> Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/file"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1; 65]))
        .mount(&server)
        .await;
    let response = client()
        .get(format!("{}/file", server.uri()))
        .send()
        .await?;
    assert!(bounded_bytes(response, 64).await.is_err());
    let response = client()
        .get(format!("{}/file", server.uri()))
        .send()
        .await?;
    assert_eq!(bounded_bytes(response, 65).await?.len(), 65);
    Ok(())
}
#[test]
fn private_cache_never_uses_sender_filename_as_path_and_preview_is_bounded() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let route = Route {
        kind: "qq".into(),
        account: "123".into(),
        peer: "group".into(),
        sender: "alice".into(),
        group: true,
    };
    let path = cache(&config, &route, "../../report.txt", b"evidence")?;
    assert!(path.starts_with(dir.path().join("channels/media")));
    assert!(path
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.starts_with(&route.key())));
    assert_eq!(fs::metadata(&path)?.mode() & 0o777, 0o600);
    assert_eq!(
        fs::metadata(path.parent().context("parent missing")?)?.mode() & 0o777,
        0o700
    );
    let mut png = Vec::new();
    image::DynamicImage::new_rgb8(30, 20)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
    let preview = preview(&png)?;
    assert_eq!(preview.media_type, "image/jpeg");
    assert!(STANDARD.decode(preview.base64_data)?.len() < 1024 * 1024);
    assert!(preview_invalid_image().is_err());
    Ok(())
}
fn preview_invalid_image() -> Result<crate::llm::ToolAttachment> {
    preview(b"not an image")
}
#[test]
fn saved_history_omits_input_image_bytes() -> Result<()> {
    use crate::llm::{ConversationItem, ConversationMessage, Role};
    let mut user = ConversationMessage::new(Role::User, "inspect screenshot");
    user.attachments.push(crate::llm::ToolAttachment {
        media_type: "image/png".into(),
        base64_data: "c2Vuc2l0aXZl".into(),
    });
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let store = crate::sessions::SessionStore::open(&config)?;
    store.save_redacted(
        "channel-test",
        &[vec![ConversationItem::Message(user)]],
        64000,
        &[],
    )?;
    let loaded = store.load("channel-test", 16, 64000)?;
    assert!(!serde_json::to_string(&loaded)?.contains("c2Vuc2l0aXZl"));
    Ok(())
}
