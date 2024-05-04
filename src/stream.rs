use std::pin::Pin;

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use log::debug;
use openh264::decoder::{DecodedYUV, Decoder};
use openh264::formats::YUVSource;
use retina::client::{Credentials, PlayOptions, SessionOptions, SetupOptions};
use retina::codec::CodecItem;
use tracing::{trace};
use turbojpeg::OwnedBuf;
use url::Url;

use crate::CameraConfig;

pub(crate) struct Stream {
    options: InnerOptions,
    decoder: Decoder,
}

impl Stream {
    pub(crate) fn new(config: &CameraConfig) -> Result<Self> {
        let credentials = if let Some(username) = &config.username {
            Some(Credentials {
                username: username.clone(),
                password: config
                    .password
                    .as_ref()
                    .ok_or(anyhow!("username set without password"))?
                    .clone(),
            })
        } else {
            None
        };

        Ok(Self {
            options: InnerOptions {
                url: Url::parse(&config.url)?,
                credentials,
            },
            decoder: Decoder::new().context("unable to instantiate decoder")?,
        })
    }

    pub(crate) async fn next(&mut self) -> Result<Vec<u8>> {
        let url = self.options.url.clone();
        debug!("connecting to: {}", url);
        let mut session = retina::client::Session::describe(url, (&self.options).into()).await?;

        trace!("streams: {:?}", session.streams());

        let video_i = session
            .streams()
            .iter()
            .position(|s| s.media() == "video" && s.encoding_name() == "h264")
            .ok_or_else(|| anyhow!("no H264 stream"))?;

        let setup_options = SetupOptions::default();

        session.setup(video_i, setup_options).await?;

        let mut session = session.play(PlayOptions::default()).await?.demuxed()?;

        let frame = loop {
            let mut packet_buffer = Vec::new();

            match Pin::new(&mut session).next().await {
                None => bail!("stream closed before first frame"),
                Some(Err(e)) => bail!("unable to get first frame: {:?}", e),
                Some(Ok(CodecItem::VideoFrame(v))) => {
                    if v.is_random_access_point() {
                        let mut jpeg = None;
                        // attempt to decode
                        for packet in avcc_to_annex_b_iterator(v.data()) {
                            //prepend the nal header to the frame.
                            packet_buffer.clear();
                            packet_buffer.reserve(packet.len() + 3);
                            packet_buffer.extend_from_slice(&[0, 0, 1]);
                            packet_buffer.extend_from_slice(packet);

                            if let Some(frame) = self
                                .decoder
                                .decode(&packet_buffer)
                                .context("corrupted video packet")?
                            {
                                // we've decoded a complete frame.
                                jpeg = Some(to_jpeg(frame).context("error converting to jpeg")?);
                                break;
                            }
                        }

                        if let Some(jpeg) = jpeg {
                            break jpeg;
                        }
                    }
                }

                Some(Ok(i)) => {
                    trace!("{:?}", i);
                }
            }
        };

        Ok(frame.to_vec())
    }
}

pub fn to_jpeg(frame: DecodedYUV) -> Result<OwnedBuf> {
    let (width, height) = frame.dimensions();
    let (stride_y, stride_u, stride_v) = frame.strides();

    trace!("dimensions: {width}, {height}");
    trace!("strides: {:?}", frame.strides());

    let mut data = Vec::with_capacity(frame.y().len() + frame.u().len() + frame.v().len());

    for row in frame.y().chunks_exact(stride_y) {
        data.extend_from_slice(&row[..width]);
    }

    for row in frame.u().chunks_exact(stride_u) {
        data.extend_from_slice(&row[..width / 2]) // 2x2 sampling
    }

    for row in frame.v().chunks_exact(stride_v) {
        data.extend_from_slice(&row[..width / 2]) // 2x2 sampling
    }

    let image = turbojpeg::YuvImage {
        pixels: data.as_slice(),
        width,
        height,
        align: 1,
        subsamp: turbojpeg::Subsamp::Sub2x2,
    };

    let (uv_width, uv_height) = frame.dimensions_uv();

    assert_eq!(image.uv_width(), uv_width);
    assert_eq!(image.uv_height(), uv_height);
    assert_eq!(
        image.y_width() * image.y_height(),
        frame.y().len() / stride_y * width
    );

    trace!("image.align: {}", image.align);
    trace!(
        "image.y_width(): {}, image.y_height(): {} ",
        image.y_width(),
        image.y_height()
    );
    trace!("result.y().len(): {}", frame.y().len());
    trace!("result.u().len(): {}", frame.u().len());
    trace!("result.v().len(): {}", frame.v().len());
    trace!(
        "assert_eq!({}, {})",
        image.y_width() * image.y_height(),
        frame.y().len() / stride_y * width
    );

    turbojpeg::compress_yuv(image, 90).context("compression_error")
}

/// Converts an avcc-formatted data frame into the annex b format *without* the nal header.
pub fn avcc_to_annex_b_iterator(mut stream: &[u8]) -> impl Iterator<Item = &[u8]> {
    std::iter::from_fn(move || {
        let mut nal_length_bytes = [0u8; 4];

        if std::io::Read::read(&mut stream, &mut nal_length_bytes).unwrap_or(0)
            == nal_length_bytes.len()
        {
            let nal_length = u32::from_be_bytes(nal_length_bytes) as usize;

            let result = &stream[..nal_length];

            stream = &stream[nal_length..];

            Some(result)
        } else {
            None
        }
    })
}

struct InnerOptions {
    url: Url,
    credentials: Option<Credentials>,
}

impl From<&InnerOptions> for SessionOptions {
    fn from(value: &InnerOptions) -> Self {
        SessionOptions::default().creds(value.credentials.clone())
    }
}
