#[cfg(feature="__io__")]
use {
    crate::io::{AsyncRead, AsyncWrite},
    crate::frame::{Frame, OpCode},
    crate::Config,
    std::io::{Error, ErrorKind},
};

#[derive(Debug)]
pub enum Message {
    Text  (String),
    Binary(Vec<u8>),
    Ping  (Vec<u8>),
    Pong  (Vec<u8>),
    Close (Option<CloseFrame>),
}

#[derive(Debug)]
pub struct CloseFrame {
    pub code:   CloseCode,
    pub reason: Option<std::borrow::Cow<'static, str>>,
}
#[derive(Debug)]
pub enum CloseCode {
    Normal, Away, Protocol, Unsupported, Status, Abnormal, Invalid,
    Policy, Size, Extension, Error, Restart, Again, Tls, Reserved,
    Iana(u16), Library(u16), Bad(u16),
}

const _: (/* trait impls */) = {
    impl From<&str> for Message {
        fn from(string: &str) -> Self {
            Self::Text(string.to_string())
        }
    }
    impl From<String> for Message {
        fn from(string: String) -> Self {
            Self::Text(string)
        }
    }
    impl From<&[u8]> for Message {
        fn from(data: &[u8]) -> Self {
            Self::Binary(data.to_vec())
        }
    }
    impl From<Vec<u8>> for Message {
        fn from(data: Vec<u8>) -> Self {
            Self::Binary(data)
        }
    }

    impl From<u16> for CloseCode {
        fn from(value: u16) -> Self {
            Self::from_u16(value)
        }
    }
    impl Into<u16> for CloseCode {
        fn into(self) -> u16 {
            self.as_u16()
        }
    }
};

impl CloseCode {
    pub const fn from_u16(u16: u16) -> Self {
        match u16 {
            1000 => Self::Normal, 1001 => Self::Away,      1002 => Self::Protocol, 1003 => Self::Unsupported,
            1005 => Self::Status, 1006 => Self::Abnormal,  1007 => Self::Invalid,  1008 => Self::Policy,
            1009 => Self::Size,   1010 => Self::Extension, 1011 => Self::Error,    1012 => Self::Restart,
            1013 => Self::Again,  1015 => Self::Tls,       1016..=2999 => Self::Reserved,
            3000..=3999 => Self::Iana(u16),
            4000..=4999 => Self::Library(u16),
            _ => Self::Bad(u16),
        }
    }
    pub const fn as_u16(&self) -> u16 {
        match self {
            Self::Normal => 1000, Self::Away      => 1001, Self::Protocol => 1002, Self::Unsupported => 1003,
            Self::Status => 1005, Self::Abnormal  => 1006, Self::Invalid  => 1007, Self::Policy      => 1008,
            Self::Size   => 1009, Self::Extension => 1010, Self::Error    => 1011, Self::Restart     => 1012,
            Self::Again  => 1013, Self::Tls       => 1015,
            Self::Reserved => 1016,
            Self::Iana(code) | Self::Library(code) | Self::Bad(code) => *code,
        }
    }
}

#[cfg(feature="__io__")]
impl CloseCode {
    pub(super) fn from_bytes(bytes: [u8; 2]) -> Self {
        Self::from(u16::from_be_bytes(bytes))
    }

    #[inline]
    pub(super) fn into_bytes(self) -> [u8; 2] {
        u16::to_be_bytes(self.as_u16())
    }
}

#[cfg(feature="__io__")]
impl Message {
    const PING_PONG_PAYLOAD_LIMIT: usize = 125;

    #[inline]
    pub(crate) fn into_frame(self) -> Frame {
        let (opcode, payload) = match self {
            Message::Text  (text)  => (OpCode::Text,   text.into_bytes()),
            Message::Binary(bytes) => (OpCode::Binary, bytes),
            Message::Ping(mut bytes) => {
                bytes.truncate(Self::PING_PONG_PAYLOAD_LIMIT);
                (OpCode::Ping, bytes)
            }
            Message::Pong(mut bytes) => {
                bytes.truncate(Self::PING_PONG_PAYLOAD_LIMIT);
                (OpCode::Ping, bytes)
            }
            Message::Close(close_frame) => {
                let payload = close_frame
                    .map(|CloseFrame { code, reason }| {
                        let code   = code.into_bytes();
                        let reason = reason.as_ref().map(|cow| cow.as_bytes()).unwrap_or(&[]);
                        [&code, reason].concat()
                    }).unwrap_or(Vec::new());
                (OpCode::Close, payload)
            }
        };

        Frame { is_final:true, opcode, payload }
    }

    #[inline]
    pub(crate) async fn write(self,
        stream: &mut (impl AsyncWrite + Unpin),
        config: &Config,
    ) -> Result<usize, Error> {
        self.into_frame().write_unmasked(stream, config).await
    }
    
    /// Read a `Message` from a WebSocket connection.
    pub(crate) async fn read_from(
        stream: &mut (impl AsyncRead + Unpin),
        config: &Config,
    ) -> Result<Option<Self>, Error> {
        let Some(first_frame) = Frame::read_from(stream, config).await? else {
            return Ok(None)
        };

        match &first_frame.opcode {
            OpCode::Text => {
                let mut payload = String::from_utf8(first_frame.payload)
                    .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Text frame's payload is not valid UTF-8: {e}")))?;
                if !first_frame.is_final {
                    while let Ok(Some(next_frame)) = Frame::read_from(stream, config).await {
                        if next_frame.opcode != OpCode::Continue {
                            return Err(Error::new(ErrorKind::InvalidData, "Expected continue frame"));
                        }
                        payload.push_str(std::str::from_utf8(&next_frame.payload)
                            .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Text frame's payload is not valid UTF-8: {e}")))?
                        );
                        if next_frame.is_final {
                            break
                        }
                    }
                }

                if let Some(limit) = &config.max_message_size {
                    (&payload.len() <= limit).then_some(())
                        .ok_or_else(|| Error::new(
                            ErrorKind::InvalidData,
                            format!("Incoming message (size: {}) is larger than limit ({})", payload.len(), *limit)
                        ))?;
                }

                Ok(Some(Message::Text(payload)))
            }
            OpCode::Binary => {
                let mut payload = first_frame.payload;
                if !first_frame.is_final {
                    while let Ok(Some(mut next_frame)) = Frame::read_from(stream, config).await {
                        if next_frame.opcode != OpCode::Continue {
                            return Err(Error::new(ErrorKind::InvalidData, "Expected continue frame"));
                        }
                        payload.append(
                            &mut next_frame.payload
                        );
                        if next_frame.is_final {
                            break
                        }
                    }
                }

                if let Some(limit) = &config.max_message_size {
                    (&payload.len() <= limit).then_some(())
                        .ok_or_else(|| Error::new(
                            ErrorKind::InvalidData,
                            format!("Incoming message (size: {}) is larger than limit ({})", payload.len(), *limit)
                        ))?;
                }

                Ok(Some(Message::Binary(payload)))
            }

            OpCode::Ping => {
                let payload = first_frame.payload;
                (payload.len() <= Self::PING_PONG_PAYLOAD_LIMIT).then_some(())
                    .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Incoming ping payload is too large"))?;
                Ok(Some(Message::Ping(payload)))
            }
            OpCode::Pong => {
                let payload = first_frame.payload;
                (payload.len() <= Self::PING_PONG_PAYLOAD_LIMIT)
                    .then_some(Some(Message::Pong(payload)))
                    .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Incoming pong payload is too large"))
            }

            OpCode::Close => {
                let payload = first_frame.payload;
                Ok(Some(Message::Close(
                    (! payload.is_empty()).then(|| {
                        let (code_bytes, rem) = payload.split_at(2);
                        let code   = CloseCode::from_bytes(unsafe {(code_bytes.as_ptr() as *const [u8; 2]).read()});
                        let reason = (! rem.is_empty()).then(|| String::from_utf8(rem.to_vec()).unwrap().into());
                        CloseFrame { code, reason }
                    })
                )))
            }

            OpCode::Continue => Err(Error::new(ErrorKind::InvalidData, "Unexpected continue frame"))
        }
    }
}
