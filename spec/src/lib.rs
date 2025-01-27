#![doc(html_root_url = "http://docs.rs/qapi-spec/0.2.2")]

use std::{io, error, fmt};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_derive::{Deserialize, Serialize};

pub use serde_json::Value as Any;
pub type Dictionary = serde_json::Map<String, Any>;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Empty { }

#[doc(hidden)]
pub mod base64 {
    use serde::{Serialize, Serializer, Deserialize, Deserializer};
    use serde::de::{Error, Unexpected};
    use base64::{self, DecodeError};

    pub fn serialize<S: Serializer>(data: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        base64::encode(data).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        // TODO: deserialize to borrowed &str
        let str = String::deserialize(deserializer)?;

        base64::decode(&str)
            .map_err(|e| de_err(&str, e))
    }

    pub fn de_err<E: Error>(str: &str, err: DecodeError) -> E {
        match err {
            DecodeError::InvalidByte(..) =>
                E::invalid_value(Unexpected::Str(str), &"base64"),
            DecodeError::InvalidLength =>
                E::invalid_length(str.len(), &"valid base64 length"),
            DecodeError::InvalidLastSymbol(..) =>
                E::invalid_value(Unexpected::Str(str), &"truncated or corrupted base64"),
        }
    }
}

#[doc(hidden)]
pub mod base64_opt {
    use serde::{Serializer, Deserialize, Deserializer};
    use crate::base64;
    use ::base64 as b64;

    pub fn serialize<S: Serializer>(data: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error> {
        base64::serialize(data.as_ref().expect("use skip_serializing_with"), serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error> {
        // TODO: deserialize to borrowed &str
        let str = <Option<String>>::deserialize(deserializer)?;
        if let Some(ref str) = str {
            b64::decode(str)
                .map(Some)
                .map_err(|e| base64::de_err(str, e))
        } else {
            Ok(None)
        }
    }
}

mod error_serde {
    use serde_derive::{Serialize, Deserialize};
    use serde::{Serialize, Serializer, Deserialize, Deserializer};
    use crate::{Error, ErrorClass, Any};

    #[derive(Deserialize)]
    pub struct ErrorValue {
        pub class: ErrorClass,
        pub desc: String,
    }

    #[derive(Deserialize)]
    struct QapiError {
        error: ErrorValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<Any>,
    }

    #[derive(Serialize)]
    pub struct ErrorValueSer<'a> {
        pub class: &'a ErrorClass,
        pub desc: &'a str,
    }

    #[derive(Serialize)]
    struct QapiErrorSer<'a> {
        error: ErrorValueSer<'a>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<&'a Any>,
    }

    impl Serialize for Error {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            QapiErrorSer {
                error: ErrorValueSer {
                    class: &self.class,
                    desc: &self.desc[..],
                },
                id: self.id.as_ref(),
            }.serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for Error {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            QapiError::deserialize(deserializer).map(|e| Error {
                class: e.error.class,
                desc: e.error.desc,
                id: e.id,
            })
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseValue<C> {
    #[serde(rename = "return")]
    return_: C,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<Any>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response<C> {
    Err(Error),
    Ok(ResponseValue<C>),
}

impl<C> Response<C> {
    pub fn result(self) -> Result<C, Error> {
        match self {
            Response::Ok(ResponseValue { return_, .. }) => Ok(return_),
            Response::Err(e) => Err(e),
        }
    }
}

pub trait Command: Serialize {
    type Ok: DeserializeOwned;

    const NAME: &'static str;
}

pub trait Event: DeserializeOwned {
    const NAME: &'static str;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ErrorClass {
    /// this is used for errors that don’t require a specific error class. This should be the default case for most errors
    GenericError,
    /// the requested command has not been found
    CommandNotFound,
    /// a device has failed to be become active
    DeviceNotActive,
    /// the requested device has not been found
    DeviceNotFound,
    /// the requested operation can’t be fulfilled because a required KVM capability is missing
    KVMMissingCap,
}

impl From<ErrorClass> for io::ErrorKind {
    fn from(e: ErrorClass) -> Self {
        match e {
            ErrorClass::GenericError => io::ErrorKind::Other,
            ErrorClass::CommandNotFound => io::ErrorKind::InvalidInput,
            ErrorClass::DeviceNotActive => io::ErrorKind::Other,
            ErrorClass::DeviceNotFound => io::ErrorKind::NotFound,
            ErrorClass::KVMMissingCap => io::ErrorKind::Other,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Error {
    pub class: ErrorClass,
    pub desc: String,
    pub id: Option<Any>,
}

impl error::Error for Error {
    fn description(&self) -> &str {
        &self.desc
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.desc, f)
    }
}

impl From<Error> for io::Error {
    fn from(e: Error) -> Self {
        io::Error::new(e.class.into(), e.desc)
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Timestamp {
    seconds: u64,
    microseconds: u64,
}

mod serde_command {
    use serde_derive::Serialize;
    use serde::{Serialize, Serializer};
    use crate::{Command, Any};

    #[derive(Serialize)]
    struct QapiCommand<'a, C: 'a> {
        execute: &'static str,
        arguments: &'a C,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<Any>,
        #[serde(skip_serializing_if = "Option::is_none")]
        control: Option<Any>,
    }

    pub fn serialize<C: Command, S: Serializer>(data: &C, serializer: S) -> Result<S::Ok, S::Error> {
        QapiCommand {
            execute: C::NAME,
            arguments: data,
            id: None,
            control: None,
        }.serialize(serializer)
    }
}

mod serde_command_ref {
    use serde::Serializer;
    use crate::{serde_command, Command};

    pub fn serialize<C: Command, S: Serializer>(data: &&C, serializer: S) -> Result<S::Ok, S::Error> {
        serde_command::serialize(*data, serializer)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandSerializer<C: Command>(#[serde(with = "serde_command")] pub C);

#[derive(Debug, Clone, Serialize)]
pub struct CommandSerializerRef<'a, C: Command + 'a>(#[serde(with = "serde_command_ref")] pub &'a C);
