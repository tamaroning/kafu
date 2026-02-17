use image::{DynamicImage, ImageReader, RgbImage};
use thiserror::Error;
use wiggle::GuestError;

pub use generated::kafu_helper::add_to_linker;

pub(crate) type KafuResult<T> = std::result::Result<T, KafuError>;

#[derive(Debug, Default)]
pub struct KafuHelperCtx {}

impl KafuHelperCtx {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Error)]
pub enum KafuError {
    #[error("guest error")]
    GuestError(#[from] GuestError),
    #[error("not enough memory: requested {0} bytes")]
    NotEnoughMemory(usize),
}

// `wiggle::from_witx!` generates functions that may legitimately exceed clippy's argument limit.
#[allow(clippy::too_many_arguments)]
mod generated {
    use super::*;
    wiggle::from_witx!({
        witx: ["../../witx/kafu.witx"],
        errors: { kafu_errno => KafuError }
    });

    /// Additionally, we must let Wiggle know which of our error codes
    /// represents a successful operation.
    impl wiggle::GuestErrorType for types::KafuErrno {
        fn success() -> Self {
            Self::Success
        }
    }

    // Convert the host errors to their WITX-generated type.
    impl types::UserErrorConversion for KafuHelperCtx {
        fn kafu_errno_from_kafu_error(&mut self, e: KafuError) -> anyhow::Result<types::KafuErrno> {
            tracing::debug!("host error: {:?}", e);
            match e {
                KafuError::GuestError(_) => unimplemented!("guest error conversion"),
                KafuError::NotEnoughMemory(_) => Ok(types::KafuErrno::TooLarge),
            }
        }
    }
}

impl generated::kafu_helper::KafuHelper for KafuHelperCtx {
    fn image_to_tensor(
        &mut self,
        mem: &mut wiggle::GuestMemory<'_>,
        image_path: wiggle::GuestPtr<str>,
        height: u32,
        width: u32,
        out_buffer: wiggle::GuestPtr<u8>,
    ) -> KafuResult<generated::types::BufferSize> {
        let image_path = mem.as_str(image_path)?.unwrap_or_default();

        let pixels = ImageReader::open(image_path).unwrap().decode().unwrap();
        let dyn_img: DynamicImage = pixels.resize_exact(width, height, image::imageops::Triangle);
        let bgr_img: RgbImage = dyn_img.to_rgb8();

        // Get an array of the pixel values
        let raw_u8_arr: &[u8] = &bgr_img.as_raw()[..];

        // Create an array to hold the f32 value of those pixels
        let bytes_required = raw_u8_arr.len() * 4;
        let mut u8_f32_arr: Vec<u8> = vec![0; bytes_required];

        // Normalizing values for the model
        let mean = [0.485, 0.456, 0.406];
        let std = [0.229, 0.224, 0.225];

        // Read the number as a f32 and break it into u8 bytes
        for i in 0..raw_u8_arr.len() {
            let u8_f32: f32 = raw_u8_arr[i] as f32;
            let rgb_iter = i % 3;

            // Normalize the pixel
            let norm_u8_f32: f32 = (u8_f32 / 255.0 - mean[rgb_iter]) / std[rgb_iter];

            // Convert it to u8 bytes and write it with new shape
            let u8_bytes = norm_u8_f32.to_ne_bytes();
            for j in 0..4 {
                u8_f32_arr[(raw_u8_arr.len() * 4 * rgb_iter / 3) + (i / 3) * 4 + j] = u8_bytes[j];
            }
        }

        let out_buffer_array = out_buffer.as_array(bytes_required as u32);
        mem.copy_from_slice(&u8_f32_arr, out_buffer_array)?;

        Ok(bytes_required as u32)
    }
}
