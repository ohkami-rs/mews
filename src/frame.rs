use std::io::{Error, ErrorKind};
use crate::runtime::{Read, Write};
use crate::Config;


#[derive(Debug, PartialEq)]
pub(crate) enum OpCode {
    /* data op codes */
    Continue /* 0x0 */,
    Text     /* 0x1 */,
    Binary   /* 0x2 */,
    /* control op codes */
    Close    /* 0x8 */,
    Ping     /* 0x9 */,
    Pong     /* 0xa */,
    /* reserved op codes */
    // Reserved /* 0x[3-7,b-f] */,
}
impl OpCode {
    #[inline]
    fn from_byte(byte: u8) -> Result<Self, Error> {
        Ok(match byte {
            0x0 => Self::Continue, 0x1 => Self::Text, 0x2 => Self::Binary,
            0x8 => Self::Close,    0x9 => Self::Ping, 0xa => Self::Pong,
            0x3..=0x7 | 0xb..=0xf => return Err(Error::new(
                ErrorKind::Unsupported, "Ohkami doesn't handle reserved opcodes")),
            _ => return Err(Error::new(
                ErrorKind::InvalidData, "OpCode out of range")),
        })
    }

    #[inline]
    const fn into_byte(self) -> u8 {
        match self {
            Self::Continue => 0x0, Self::Text => 0x1, Self::Binary => 0x2,
            Self::Close    => 0x8, Self::Ping => 0x9, Self::Pong   => 0xa,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Frame {
    pub(crate) is_final: bool,
    pub(crate) opcode:   OpCode,
    pub(crate) payload:  Vec<u8>,
}

#[cfg(feature="__runtime__")]
impl Frame {
    pub(crate) async fn read_from(
        stream: &mut (impl Read + Unpin),
        config: &Config,
    ) -> Result<Option<Self>, Error> {
        let [first, second] = {
            let mut head = [0; 2];
            stream.read_exact(&mut head).await?;
            head
        };

        let is_final = first & 0x80 != 0;
        let opcode   = OpCode::from_byte(first & 0x0F)?;

        let payload_len = {
            let payload_len_byte = second & 0x7F;
            let len_part_size = match payload_len_byte {127=>8, 126=>2, _=>0};

            let len = match len_part_size {
                0 => payload_len_byte as usize,
                _ => {
                    let mut bytes = [0; 8];
                    if let Err(e) = stream.read_exact(&mut bytes[(8 - len_part_size)..]).await {
                        return match e.kind() {
                            ErrorKind::UnexpectedEof => Ok(None),
                            _                        => Err(e.into()),
                        }
                    }
                    usize::from_be_bytes(bytes)
                }
            }; if let Some(limit) = &config.max_frame_size {
                (&len <= limit).then_some(())
                    .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Incoming frame is too large"))?;
            }

            len
        };

        let mask = if second & 0x80 == 0 {
            (config.accept_unmasked_frames).then_some(None)
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "Client frame is unmasked"))?
        } else {
            let mut mask_bytes = [0; 4];
            if let Err(e) = stream.read_exact(&mut mask_bytes).await {
                return match e.kind() {
                    ErrorKind::UnexpectedEof => Ok(None),
                    _                        => Err(e.into()),
                }
            }
            Some(mask_bytes)
        };

        let payload = {
            let mut payload = vec![0u8; payload_len];
            stream.read_exact(&mut payload).await?;

            if let Some(masking_bytes) = mask {
                let mut i = 0;
                for b in &mut payload {
                    *b = *b ^ masking_bytes[i];

                    /*
                    i = if i == 3 {0} else {i + 1};
                    */
                    i = (i + 1) & 0b00000011;
                }
            }

            payload
        };

        Ok(Some(Self { is_final, opcode, payload }))
    }

    pub(crate) async fn write_unmasked(self,
        stream:  &mut (impl Write + Unpin),
        _config: &Config,
    ) -> Result<usize, Error> {
        fn into_bytes(frame: Frame) -> Vec<u8> {
            let Frame { is_final, opcode, payload } = frame;

            let (payload_len_byte, payload_len_bytes) = match payload.len() {
                ..=125      => (payload.len() as u8, None),
                126..=65535 => (126, Some((|len: u16| len.to_be_bytes().to_vec())(payload.len() as u16))),
                _           => (127, Some((|len: u64| len.to_be_bytes().to_vec())(payload.len() as u64))),
            };

            let first  = ((is_final as u8) << 7) + opcode.into_byte();
            let second = (0/* MASK: off */ << 7) + payload_len_byte;

            let mut header_bytes = vec![first, second];
            if let Some(mut payload_len_bytes) = payload_len_bytes {
                header_bytes.append(&mut payload_len_bytes)
            }

            [header_bytes, payload].concat()
        }

        stream.write(&into_bytes(self)).await
    }
}
