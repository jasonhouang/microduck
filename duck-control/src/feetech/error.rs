use thiserror::Error;

#[derive(Error, Debug)]
pub enum ServoError {
    #[error("serial port: {0}")]
    Serial(#[from] serialport::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("servo did not answer within timeout")]
    Timeout,

    #[error("checksum mismatch")]
    Checksum,

    #[error("malformed frame: {0}")]
    Malformed(&'static str),

    #[error("servo status 0x{0:02X}: {1}")]
    ServoStatus(u8, String),

    #[error("invalid parameter: {0}")]
    InvalidParam(String),
}

pub type Result<T> = std::result::Result<T, ServoError>;
