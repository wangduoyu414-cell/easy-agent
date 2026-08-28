use std::io::Read;

use flate2::read::DeflateDecoder;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WebInstallerExpectation<'a> {
    pub store_id: &'a str,
    pub package_family: &'a str,
}

#[derive(Debug, Deserialize)]
struct WebInstallerTag {
    schema: u32,
    expires: u64,
    #[serde(rename = "productId")]
    product_id: String,
    #[serde(rename = "installerType")]
    installer_type: String,
    #[serde(rename = "pfns")]
    package_family_names: Vec<String>,
    #[serde(rename = "autoUpdate")]
    auto_update: bool,
    #[serde(rename = "isHarbor")]
    is_harbor: bool,
}

pub(crate) fn extract_ms_store_tag(bytes: &[u8]) -> Result<Vec<u8>, String> {
    const OID_DER: &[u8] = &[
        0x06, 0x0b, 0x2b, 0x06, 0x01, 0x04, 0x01, 0xd6, 0x79, 0x02, 0x01, 0xce, 0x0f,
    ];
    let oid = find_bytes(bytes, OID_DER)
        .ok_or_else(|| "微软安装器缺少 Microsoft Store 产品绑定".to_owned())?;
    let tail = &bytes[oid + OID_DER.len()..];
    let octet = tail
        .windows(4)
        .position(|window| window == [0x04, 0x82, 0x40, 0x00])
        .ok_or_else(|| "微软安装器 Store 标签格式异常".to_owned())?;
    let start = octet + 4;
    let end = start + 16 * 1024;
    if end > tail.len() {
        return Err("微软安装器 Store 标签被截断".into());
    }
    let tag = tail[start..end].to_vec();
    if !tag.starts_with(b"MSStoreTag001") {
        return Err("微软安装器 Store 标签头不匹配".into());
    }
    Ok(tag)
}

pub(crate) fn validate_web_installer_tag(
    tag: &[u8],
    expectation: &WebInstallerExpectation<'_>,
) -> Result<(), String> {
    const HEADER: &[u8] = b"MSStoreTag001";
    if !tag.starts_with(HEADER) || tag.len() < HEADER.len() + 8 {
        return Err("微软安装器 Store 标签格式异常".into());
    }
    let mut offset = HEADER.len();
    let signature_length = read_tag_i32(tag, offset, "签名长度")?;
    offset += 4;
    if signature_length >= 16 * 1024 || offset + signature_length > tag.len() {
        return Err("微软安装器 Store 标签签名长度异常".into());
    }
    offset += signature_length;
    let payload_length = read_tag_i32(tag, offset, "配置长度")?;
    offset += 4;
    if payload_length >= 16 * 1024 || offset + payload_length > tag.len() {
        return Err("微软安装器 Store 标签配置长度异常".into());
    }
    let decoder = DeflateDecoder::new(&tag[offset..offset + payload_length]);
    let mut json = Vec::new();
    decoder
        .take(256 * 1024)
        .read_to_end(&mut json)
        .map_err(|error| format!("无法解压微软安装器产品配置：{error}"))?;
    let parsed: WebInstallerTag = serde_json::from_slice(&json)
        .map_err(|error| format!("微软安装器产品配置无效：{error}"))?;
    if parsed.schema < 4
        || parsed.expires == 0
        || parsed.product_id != expectation.store_id
        || parsed.installer_type != "WindowsUpdate"
        || parsed.package_family_names.as_slice() != [expectation.package_family]
        || parsed.auto_update
        || !parsed.is_harbor
    {
        return Err("微软安装器的产品、Package Family 或安装模式与 ChatGPT 固定合同不一致".into());
    }
    Ok(())
}

fn read_tag_i32(tag: &[u8], offset: usize, label: &str) -> Result<usize, String> {
    let bytes: [u8; 4] = tag
        .get(offset..offset + 4)
        .ok_or_else(|| format!("微软安装器 Store 标签缺少{label}"))?
        .try_into()
        .expect("length checked");
    let value = i32::from_le_bytes(bytes);
    if value <= 0 {
        return Err(format!("微软安装器 Store 标签{label}非法"));
    }
    Ok(value as usize)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::DeflateEncoder;

    use super::*;

    fn fixture_tag(product_id: &str, family: &str) -> Vec<u8> {
        let json = format!(
            r#"{{"schema":4,"expires":1786527229,"productId":"{product_id}","installerType":"WindowsUpdate","pfns":["{family}"],"autoUpdate":false,"isHarbor":true}}"#
        );
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(json.as_bytes()).unwrap();
        let payload = encoder.finish().unwrap();
        let mut tag = vec![0_u8; 16 * 1024];
        let mut offset = 0;
        tag[..13].copy_from_slice(b"MSStoreTag001");
        offset += 13;
        tag[offset..offset + 4].copy_from_slice(&(256_i32).to_le_bytes());
        offset += 4 + 256;
        tag[offset..offset + 4].copy_from_slice(&(payload.len() as i32).to_le_bytes());
        offset += 4;
        tag[offset..offset + payload.len()].copy_from_slice(&payload);
        tag
    }

    #[test]
    fn parses_and_validates_the_fixed_chatgpt_product_contract() {
        let expectation = WebInstallerExpectation {
            store_id: "9PLM9XGG6VKS",
            package_family: "OpenAI.Codex_2p2nqsd0c76g0",
        };
        validate_web_installer_tag(
            &fixture_tag(expectation.store_id, expectation.package_family),
            &expectation,
        )
        .unwrap();
        assert!(
            validate_web_installer_tag(
                &fixture_tag("OTHERPRODUCT", expectation.package_family),
                &expectation
            )
            .is_err()
        );
    }

    #[test]
    fn extracts_the_extension_payload_from_a_certificate_blob() {
        let tag = fixture_tag("9PLM9XGG6VKS", "OpenAI.Codex_2p2nqsd0c76g0");
        let mut bytes = b"header".to_vec();
        bytes.extend_from_slice(&[
            0x06, 0x0b, 0x2b, 0x06, 0x01, 0x04, 0x01, 0xd6, 0x79, 0x02, 0x01, 0xce, 0x0f, 0x04,
            0x82, 0x40, 0x00,
        ]);
        bytes.extend_from_slice(&tag);
        assert_eq!(extract_ms_store_tag(&bytes).unwrap(), tag);
    }
}
