use std::{error,
          fmt,
          io,
          result};

use reqwest;
use serde_json;
use url;

use habitat_core as hab_core;

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    HabitatCore(hab_core::Error),
    ReqwestError(reqwest::Error),
    IO(io::Error),
    Json(serde_json::Error),
    UrlParseError(url::ParseError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg = match *self {
            Error::HabitatCore(ref e) => format!("{}", e),
            Error::ReqwestError(ref err) => format!("{}", err),
            Error::IO(ref e) => format!("{}", e),
            Error::Json(ref e) => format!("{}", e),
            Error::UrlParseError(ref e) => format!("{}", e),
        };
        write!(f, "{}", msg)
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::HabitatCore(ref err) => err.description(),
            Error::ReqwestError(ref err) => err.description(),
            Error::IO(ref err) => err.description(),
            Error::Json(ref err) => err.description(),
            Error::UrlParseError(ref err) => err.description(),
        }
    }
}

impl From<hab_core::Error> for Error {
    fn from(err: hab_core::Error) -> Error { Error::HabitatCore(err) }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Error { Error::ReqwestError(err) }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error { Error::IO(err) }
}

impl From<url::ParseError> for Error {
    fn from(err: url::ParseError) -> Self { Error::UrlParseError(err) }
}
