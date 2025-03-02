// Copyright lowRISC contributors.
// Licensed under the Apache License, Version 2.0, see LICENSE for details.
// SPDX-License-Identifier: Apache-2.0

//! `KeyExchange` request and response.
//!
//! This module provides a Cerberus command allowing the versions of various
//! on-device firmware to be queried.

use core::convert::TryInto as _;

use crate::io::read::ReadZeroExt as _;
use crate::io::ReadInt as _;
use crate::io::ReadZero;
use crate::io::Write;
use crate::mem::Arena;
use crate::protocol::wire;
use crate::protocol::wire::FromWire;
use crate::protocol::wire::ToWire;
use crate::protocol::Command;
use crate::protocol::CommandType;
use crate::protocol::Request;
use crate::protocol::Response;

#[cfg(feature = "arbitrary-derive")]
use libfuzzer_sys::arbitrary::{self, Arbitrary};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use crate::hardware::Identity;

/// A command for requesting a firmware version.
///
/// Corresponds to [`CommandType::KeyExchange`].
///
/// See [`Identity::firmware_version()`].
pub enum KeyExchange {}

impl<'wire> Command<'wire> for KeyExchange {
    type Req = KeyExchangeRequest<'wire>;
    type Resp = KeyExchangeResponse<'wire>;
}

wire_enum! {
    /// A type of key exchange request.
    enum RequestType: u8 {
        /// A request to establish a shared session key.
        SessionKey = 0x00,

        /// A request to compare pairing keys, or generate them if
        /// this is the first time.
        PairedKeyHmac = 0x01,

        /// A request to destroy an encrypted session.
        DestroySession = 0x02,
    }
}

make_fuzz_safe! {
    /// The [`KeyExchange`] request.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
    pub enum KeyExchangeRequest<'wire> {
        /// A request to establish a shared session key.
        SessionKey {
            /// The HMAC algorithm to use throughout the session.
            // TODO: This will use a proper enum, pending on a rewrite of
            // crypto::sha256.
            hmac_algorithm: u8,

            /// A DER-encoded ECDSA public key, which will be fed into
            /// the ECDH.
            #[cfg_attr(feature = "serde", serde(borrow))]
            pk_req: &'wire [u8],
        },

        /// A request to compare pairing keys, or generate them if
        /// this is the first time.
        PairedKeyHmac {
            /// The length of the pairing key, in bytes.
            key_len: usize,

            /// The HMAC of the pairing key with the session's MAC key.
            #[cfg_attr(feature = "serde", serde(borrow))]
            key_hmac: &'wire [u8],
        },

        /// A request to destroy an encrypted session.
        DestroySession {
            /// The HMAC of the session key with the session's MAC key.
            #[cfg_attr(feature = "serde", serde(borrow))]
            session_hmac: &'wire [u8],
        },
    }
}

impl<'wire> Request<'wire> for KeyExchangeRequest<'wire> {
    const TYPE: CommandType = CommandType::KeyExchange;
}

impl<'wire> FromWire<'wire> for KeyExchangeRequest<'wire> {
    fn from_wire<R: ReadZero<'wire> + ?Sized, A: Arena>(
        r: &mut R,
        arena: &'wire A,
    ) -> Result<Self, wire::Error> {
        match RequestType::from_wire(r, arena)? {
            RequestType::SessionKey => {
                let hmac_algorithm = r.read_le()?;
                let pk_len = r.remaining_data();
                let pk_req = r.read_slice(pk_len, arena)?;
                Ok(Self::SessionKey {
                    hmac_algorithm,
                    pk_req,
                })
            }
            RequestType::PairedKeyHmac => {
                let key_len = r.read_le::<u16>()? as usize;
                let hmac_len = r.remaining_data();
                let key_hmac = r.read_slice(hmac_len, arena)?;
                Ok(Self::PairedKeyHmac { key_len, key_hmac })
            }
            RequestType::DestroySession => {
                let hmac_len = r.remaining_data();
                let session_hmac = r.read_slice(hmac_len, arena)?;
                Ok(Self::DestroySession { session_hmac })
            }
        }
    }
}

impl ToWire for KeyExchangeRequest<'_> {
    fn to_wire<W: Write>(&self, mut w: W) -> Result<(), wire::Error> {
        match self {
            Self::SessionKey {
                hmac_algorithm,
                pk_req,
            } => {
                RequestType::SessionKey.to_wire(&mut w)?;
                w.write_le(*hmac_algorithm)?;
                w.write_bytes(pk_req)?;
            }
            Self::PairedKeyHmac { key_len, key_hmac } => {
                RequestType::PairedKeyHmac.to_wire(&mut w)?;
                let key_len: u16 = (*key_len)
                    .try_into()
                    .map_err(|_| wire::Error::OutOfRange)?;
                w.write_le(key_len)?;
                w.write_bytes(key_hmac)?;
            }
            Self::DestroySession { session_hmac } => {
                RequestType::DestroySession.to_wire(&mut w)?;
                w.write_bytes(session_hmac)?;
            }
        }

        Ok(())
    }
}

make_fuzz_safe! {
    /// The [`KeyExchange`] response.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
    pub enum KeyExchangeResponse<'wire> {
        /// A response to a session establishment request.
        SessionKey {
            /// A DER-encoded ECDSA public key, which will be fed into
            /// the ECDH.
            #[cfg_attr(feature = "serde", serde(borrow))]
            pk_resp: &'wire [u8],

            /// A signature over `pk_req || pk_resp`, using the previously
            /// negotiated Alias Certificate's private key.
            #[cfg_attr(feature = "serde", serde(borrow))]
            signature: &'wire [u8],

            /// The HMAC of the encoded alias cert with the session's MAC key.
            #[cfg_attr(feature = "serde", serde(borrow))]
            alias_cert_hmac: &'wire [u8],
        },
        /// A response to a paired-key HMAC request; indicates success.
        PairedKeyHmac,
        /// A response to a session-destruction request; indicates success.
        DestroySession,
    }
}

impl<'wire> Response<'wire> for KeyExchangeResponse<'wire> {
    const TYPE: CommandType = CommandType::KeyExchange;
}

impl<'wire> FromWire<'wire> for KeyExchangeResponse<'wire> {
    fn from_wire<R: ReadZero<'wire> + ?Sized, A: Arena>(
        r: &mut R,
        arena: &'wire A,
    ) -> Result<Self, wire::Error> {
        match RequestType::from_wire(r, arena)? {
            RequestType::SessionKey => {
                let pk_len = r.read_le::<u16>()? as usize;
                let pk_resp = r.read_slice(pk_len, arena)?;

                let sig_len = r.read_le::<u16>()? as usize;
                let signature = r.read_slice(sig_len, arena)?;

                let cert_len = r.remaining_data();
                let alias_cert_hmac = r.read_slice(cert_len, arena)?;
                Ok(Self::SessionKey {
                    pk_resp,
                    signature,
                    alias_cert_hmac,
                })
            }
            RequestType::PairedKeyHmac => Ok(Self::PairedKeyHmac),
            RequestType::DestroySession => Ok(Self::DestroySession),
        }
    }
}

impl ToWire for KeyExchangeResponse<'_> {
    fn to_wire<W: Write>(&self, mut w: W) -> Result<(), wire::Error> {
        match self {
            Self::SessionKey {
                pk_resp,
                signature,
                alias_cert_hmac,
            } => {
                RequestType::SessionKey.to_wire(&mut w)?;
                let pk_len: u16 = pk_resp
                    .len()
                    .try_into()
                    .map_err(|_| wire::Error::OutOfRange)?;
                w.write_le(pk_len)?;
                w.write_bytes(pk_resp)?;

                let sig_len: u16 = signature
                    .len()
                    .try_into()
                    .map_err(|_| wire::Error::OutOfRange)?;
                w.write_le(sig_len)?;
                w.write_bytes(signature)?;

                w.write_bytes(alias_cert_hmac)?;
                Ok(())
            }
            Self::PairedKeyHmac => RequestType::PairedKeyHmac.to_wire(&mut w),
            Self::DestroySession => RequestType::DestroySession.to_wire(&mut w),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    round_trip_test! {
        request_round_trip_session_key: {
            bytes: &[
                0x00,  // Type 0.
                0x00,  // SHA-256.
                b'e', b'c', b'd', b's', b'a',
            ],
            value: KeyExchangeRequest::SessionKey {
                hmac_algorithm: 0,
                pk_req: b"ecdsa",
            },
        },
        request_round_trip_pairing_key: {
            bytes: &[
                0x01,  // Type 1.
                0x02, 0x01,
                b'h', b'm', b'a', b'c',
            ],
            value: KeyExchangeRequest::PairedKeyHmac {
                key_len: 258,
                key_hmac: b"hmac",
            },
        },
        request_round_trip_destroy: {
            bytes: &[
                0x02,  // Type 2.
                b'h', b'm', b'a', b'c',
            ],
            value: KeyExchangeRequest::DestroySession {
                session_hmac: b"hmac",
            },
        },

        response_round_trip_session_key: {
            bytes: &[
                0x00,  // Type 0.
                0x05, 0x00,
                b'e', b'c', b'd', b's', b'a',
                0x09, 0x00,
                b's', b'i', b'g', b'n', b'a', b't', b'u', b'r', b'e',
                b'a', b'l', b'i', b'a', b's',
            ],
            value: KeyExchangeResponse::SessionKey {
                pk_resp: b"ecdsa",
                signature: b"signature",
                alias_cert_hmac: b"alias",
            },
        },
        response_round_trip_pairing_key: {
            bytes: &[
                0x01,  // Type 1.
            ],
            value: KeyExchangeResponse::PairedKeyHmac,
        },
        response_round_trip_destroy: {
            bytes: &[
                0x02,  // Type 2.
            ],
            value: KeyExchangeResponse::DestroySession,
        },
    }
}
