use crate::{Error, Result};
use async_io::Async;
use nix::unistd::Uid;
use std::{
    collections::HashMap, convert::TryFrom, env, ffi::OsString, os::unix::net::UnixStream,
    str::FromStr,
};

/// A `tcp:` address family.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TcpAddressFamily {
    Ipv4,
    Ipv6,
}

/// A `tcp:` D-Bus address.
#[derive(Clone, Debug, PartialEq)]
pub struct TcpAddress {
    pub(crate) host: String,
    pub(crate) bind: Option<String>,
    pub(crate) port: u16,
    pub(crate) family: Option<TcpAddressFamily>,
}

impl TcpAddress {
    /// Returns the `tcp:` address `host` value.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Returns the `tcp:` address `bind` value.
    pub fn bind(&self) -> Option<&str> {
        self.bind.as_deref()
    }

    /// Returns the `tcp:` address `port` value.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns the `tcp:` address `family` value.
    pub fn family(&self) -> Option<TcpAddressFamily> {
        self.family
    }
}

/// A bus address
#[derive(Clone, Debug, PartialEq)]
pub enum Address {
    /// A path on the filesystem
    Unix(OsString),
    /// TCP address details
    Tcp(TcpAddress),
}

#[derive(Debug)]
pub(crate) enum Stream {
    Unix(Async<UnixStream>),
}

impl Address {
    pub(crate) async fn connect(&self) -> Result<Stream> {
        match self {
            Address::Unix(p) => Async::<UnixStream>::connect(p)
                .await
                .map(Stream::Unix)
                .map_err(Error::Io),
            _ => unimplemented!(),
        }
    }

    /// Get the address for session socket respecting the DBUS_SESSION_BUS_ADDRESS environment
    /// variable. If we don't recognize the value (or it's not set) we fall back to
    /// $XDG_RUNTIME_DIR/bus
    pub fn session() -> Result<Self> {
        match env::var("DBUS_SESSION_BUS_ADDRESS") {
            Ok(val) => Self::from_str(&val),
            _ => {
                let runtime_dir = env::var("XDG_RUNTIME_DIR")
                    .unwrap_or_else(|_| format!("/run/user/{}", Uid::current()));
                let path = format!("unix:path={}/bus", runtime_dir);

                Self::from_str(&path)
            }
        }
    }

    /// Get the address for system bus respecting the DBUS_SYSTEM_BUS_ADDRESS environment
    /// variable. If we don't recognize the value (or it's not set) we fall back to
    /// /var/run/dbus/system_bus_socket
    pub fn system() -> Result<Self> {
        match env::var("DBUS_SYSTEM_BUS_ADDRESS") {
            Ok(val) => Self::from_str(&val),
            _ => Self::from_str("unix:path=/var/run/dbus/system_bus_socket"),
        }
    }

    // Helper for FromStr
    fn from_unix(opts: HashMap<&str, &str>) -> Result<Self> {
        let path = if let Some(abs) = opts.get("abstract") {
            if opts.get("path").is_some() {
                return Err(Error::Address(
                    "`path` and `abstract` cannot be specified together".into(),
                ));
            }
            let mut s = OsString::from("\0");
            s.push(abs);
            s
        } else if let Some(path) = opts.get("path") {
            OsString::from(path)
        } else {
            return Err(Error::Address(
                "unix address is missing path or abstract".to_owned(),
            ));
        };

        Ok(Address::Unix(path))
    }

    // Helper for FromStr
    fn from_tcp(opts: HashMap<&str, &str>) -> Result<Self> {
        let bind = None;
        if opts.contains_key("bind") {
            return Err(Error::Address("`bind` isn't yet supported".into()));
        }

        let host = opts
            .get("host")
            .ok_or_else(|| Error::Address("tcp address is missing `host`".into()))?
            .to_string();
        let port = opts
            .get("port")
            .ok_or_else(|| Error::Address("tcp address is missing `port`".into()))?;
        let port = port
            .parse::<u16>()
            .map_err(|_| Error::Address("invalid tcp `port`".into()))?;
        let family = opts
            .get("family")
            .map(|f| TcpAddressFamily::from_str(f))
            .transpose()?;

        Ok(Address::Tcp(TcpAddress {
            host,
            bind,
            port,
            family,
        }))
    }
}

impl FromStr for TcpAddressFamily {
    type Err = Error;

    fn from_str(family: &str) -> Result<Self> {
        match family {
            "ipv4" => Ok(Self::Ipv4),
            "ipv6" => Ok(Self::Ipv6),
            _ => Err(Error::Address(format!(
                "invalid tcp address `family`: {}",
                family
            ))),
        }
    }
}

impl FromStr for Address {
    type Err = Error;

    /// Parse a D-BUS address and return its path if we recognize it
    fn from_str(address: &str) -> Result<Self> {
        let col = address
            .find(':')
            .ok_or_else(|| Error::Address("address has no colon".into()))?;
        let transport = &address[..col];
        let mut options = HashMap::new();
        for kv in address[col + 1..].split(',') {
            let (k, v) = match kv.find('=') {
                Some(eq) => (&kv[..eq], &kv[eq + 1..]),
                None => return Err(Error::Address("missing = when parsing key/value".into())),
            };
            if options.insert(k, v).is_some() {
                return Err(Error::Address(format!(
                    "Key `{}` specified multiple times",
                    k
                )));
            }
        }

        match transport {
            "unix" => Self::from_unix(options),
            "tcp" => Self::from_tcp(options),
            _ => Err(Error::Address(format!(
                "unsupported transport '{}'",
                transport
            ))),
        }
    }
}

impl TryFrom<&str> for Address {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self> {
        Self::from_str(value)
    }
}

#[cfg(test)]
mod tests {
    use super::Address;
    use crate::{Error, TcpAddress, TcpAddressFamily};
    use std::str::FromStr;
    use test_log::test;

    #[test]
    fn parse_dbus_addresses() {
        match Address::from_str("").unwrap_err() {
            Error::Address(e) => assert_eq!(e, "address has no colon"),
            _ => panic!(),
        }
        match Address::from_str("foo").unwrap_err() {
            Error::Address(e) => assert_eq!(e, "address has no colon"),
            _ => panic!(),
        }
        match Address::from_str("foo:opt").unwrap_err() {
            Error::Address(e) => assert_eq!(e, "missing = when parsing key/value"),
            _ => panic!(),
        }
        match Address::from_str("foo:opt=1,opt=2").unwrap_err() {
            Error::Address(e) => assert_eq!(e, "Key `opt` specified multiple times"),
            _ => panic!(),
        }
        match Address::from_str("tcp:host=localhost").unwrap_err() {
            Error::Address(e) => assert_eq!(e, "tcp address is missing `port`"),
            _ => panic!(),
        }
        match Address::from_str("tcp:host=localhost,port=32f").unwrap_err() {
            Error::Address(e) => assert_eq!(e, "invalid tcp `port`"),
            _ => panic!(),
        }
        match Address::from_str("tcp:host=localhost,port=123,family=ipv7").unwrap_err() {
            Error::Address(e) => assert_eq!(e, "invalid tcp address `family`: ipv7"),
            _ => panic!(),
        }
        match Address::from_str("unix:foo=blah").unwrap_err() {
            Error::Address(e) => assert_eq!(e, "unix address is missing path or abstract"),
            _ => panic!(),
        }
        match Address::from_str("unix:path=/tmp,abstract=foo").unwrap_err() {
            Error::Address(e) => {
                assert_eq!(e, "`path` and `abstract` cannot be specified together")
            }
            _ => panic!(),
        }
        assert_eq!(
            Address::Unix("/tmp/dbus-foo".into()),
            Address::from_str("unix:path=/tmp/dbus-foo").unwrap()
        );
        assert_eq!(
            Address::Unix("/tmp/dbus-foo".into()),
            Address::from_str("unix:path=/tmp/dbus-foo,guid=123").unwrap()
        );
        assert_eq!(
            Address::Tcp(TcpAddress {
                host: "localhost".into(),
                port: 4142,
                bind: None,
                family: None
            }),
            Address::from_str("tcp:host=localhost,port=4142").unwrap()
        );
        assert_eq!(
            Address::Tcp(TcpAddress {
                host: "localhost".into(),
                port: 4142,
                bind: None,
                family: Some(TcpAddressFamily::Ipv4)
            }),
            Address::from_str("tcp:host=localhost,port=4142,family=ipv4").unwrap()
        );
        assert_eq!(
            Address::Tcp(TcpAddress {
                host: "localhost".into(),
                port: 4142,
                bind: None,
                family: Some(TcpAddressFamily::Ipv6)
            }),
            Address::from_str("tcp:host=localhost,port=4142,family=ipv6").unwrap()
        );
    }
}
