use crate::actors::storage_actor::model::unit::ImageAttachment;
use crate::actors::tools_actor::provider::ToolResult;
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use serde_json::json;
use std::io::Cursor;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 25_000_000;

/// None means a non-image file; the caller retains its existing text reader.
pub(super) async fn try_read(
    path: &Path,
    display_path: &str,
) -> Result<Option<ToolResult>, String> {
    // Opening a FIFO can block a Tokio filesystem worker even after the caller
    // times out. Reject special files before opening, then recheck the handle.
    if !tokio::fs::metadata(path)
        .await
        .map_err(|e| e.to_string())?
        .is_file()
    {
        return Err("path must be a regular file".into());
    }
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| e.to_string())?;
    let metadata = file.metadata().await.map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("path must be a regular file".into());
    }
    let mut header = [0; 32];
    let mut length = 0;
    while length < header.len() {
        let read = file
            .read(&mut header[length..])
            .await
            .map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        length += read;
    }
    let Ok(format) = image::guess_format(&header[..length]) else {
        return Ok(None);
    };
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err("unsupported image format; use PNG, JPEG or WebP".into());
    }
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err("image exceeds the 10 MiB limit".into());
    }
    file.rewind().await.map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    // Bound the read as well as metadata: the file may grow after inspection.
    file.take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err("image exceeds the 10 MiB limit".into());
    }
    let filename = display_path.to_owned();
    tokio::task::spawn_blocking(move || decode_image(bytes, format, filename))
        .await
        .map_err(|e| e.to_string())?
        .map(Some)
}

fn decode_image(
    bytes: Vec<u8>,
    format: ImageFormat,
    filename: String,
) -> Result<ToolResult, String> {
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|e| format!("invalid image: {e}"))?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err("image exceeds the 25 million pixel limit or has invalid dimensions".into());
    }
    // Validate compressed data, but preserve original bytes and image quality.
    image::DynamicImage::from_decoder(decoder).map_err(|e| format!("invalid image: {e}"))?;
    let mimetype = format.to_mime_type().to_owned();
    let mut result = ToolResult::success(json!({
        "path": filename, "mimetype": mimetype, "width": width, "height": height,
        "bytes": bytes.len(), "image_attached": true,
    }));
    result.images.push(ImageAttachment {
        mimetype,
        filename,
        width,
        height,
        data: STANDARD.encode(bytes),
    });
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::tools_actor::{
        builtin_provider::fs::execute_file_read, model::ToolExecutionContext,
    };
    use crate::handles::test_handles;

    #[tokio::test]
    async fn reads_supported_images_by_content_and_preserves_original_bytes() {
        let root = std::env::temp_dir().join(format!("prism-image-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            let mut buffer = Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(3, 2)
                .write_to(&mut buffer, format)
                .unwrap();
            let bytes = buffer.into_inner();
            // A misleading extension must not change image detection.
            let path = root.join("image.txt");
            std::fs::write(&path, &bytes).unwrap();
            let result = execute_file_read(
                ToolExecutionContext {
                    handles: test_handles(),
                    workspace_uuid: "ws".into(),
                    caller_agent_uuid: "agent".into(),
                    workspace_path: root.clone(),
                },
                json!({"path": "image.txt"}),
            )
            .await;
            assert!(!result.is_error, "{:?}", result.content);
            assert_eq!(result.images.len(), 1);
            let image = &result.images[0];
            assert_eq!(image.mimetype, format.to_mime_type());
            assert_eq!((image.width, image.height), (3, 2));
            assert_eq!(STANDARD.decode(&image.data).unwrap(), bytes);
            assert!(!result.content.to_string().contains(&image.data));
        }
        let path = root.join("plain.txt");
        std::fs::write(&path, "ordinary text").unwrap();
        assert!(try_read(&path, "plain.txt").await.unwrap().is_none());
        std::fs::write(&path, b"GIF89a").unwrap();
        assert!(
            try_read(&path, "plain.txt")
                .await
                .unwrap_err()
                .contains("unsupported")
        );
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n").unwrap();
        assert!(
            try_read(&path, "plain.txt")
                .await
                .unwrap_err()
                .contains("invalid image")
        );
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(MAX_IMAGE_BYTES + 1).unwrap();
        assert!(
            try_read(&path, "plain.txt")
                .await
                .unwrap_err()
                .contains("10 MiB")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_special_files_before_opening() {
        let path = std::env::temp_dir().join(format!("prism-socket-{}", uuid::Uuid::now_v7()));
        let socket = std::os::unix::net::UnixListener::bind(&path).unwrap();
        // Opening this socket as a file would fail with an OS error rather than
        // our regular-file validation, proving the check happens before open.
        assert_eq!(
            try_read(&path, "socket").await.unwrap_err(),
            "path must be a regular file"
        );
        drop(socket);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_excessive_dimensions_before_decoding_pixels() {
        // A tiny compressed PNG can still expand beyond the pixel budget.
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::new_luma8(5001, 5000)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        assert!(
            decode_image(bytes.into_inner(), ImageFormat::Png, "large.png".into())
                .unwrap_err()
                .contains("25 million")
        );
    }
}
