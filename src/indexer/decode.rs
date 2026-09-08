use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use stellar_xdr::{Limits, PublicKey, ReadXdr, ScAddress, ScVal};

pub fn decode_scval_base64(b64: &str) -> Result<ScVal> {
    ScVal::from_xdr_base64(b64, Limits::none())
        .with_context(|| format!("failed to decode ScVal from base64: {b64}"))
}

pub fn sc_address_to_string(addr: &ScAddress) -> Result<String> {
    match addr {
        ScAddress::Account(account_id) => {
            let PublicKey::PublicKeyTypeEd25519(bytes) = &account_id.0;
            let pk = stellar_strkey::ed25519::PublicKey(bytes.0);
            Ok(pk.to_string().to_string())
        }
        ScAddress::Contract(contract_id) => {
            let contract = stellar_strkey::Contract(contract_id.0 .0);
            Ok(contract.to_string().to_string())
        }
        other => bail!("unsupported address type: {other:?}"),
    }
}

pub fn sc_val_to_address(val: &ScVal) -> Result<String> {
    match val {
        ScVal::Address(addr) => sc_address_to_string(addr),
        other => bail!("expected Address ScVal, got {other:?}"),
    }
}

pub fn sc_val_to_u32(val: &ScVal) -> Result<u32> {
    match val {
        ScVal::U32(v) => Ok(*v),
        other => bail!("expected U32 ScVal, got {other:?}"),
    }
}

pub fn sc_val_to_u64(val: &ScVal) -> Result<u64> {
    match val {
        ScVal::U64(v) => Ok(*v),
        other => bail!("expected U64 ScVal, got {other:?}"),
    }
}

pub fn sc_val_to_i128(val: &ScVal) -> Result<i128> {
    match val {
        ScVal::I128(parts) => Ok(((parts.hi as i128) << 64) | (parts.lo as i128)),
        other => bail!("expected I128 ScVal, got {other:?}"),
    }
}

pub fn sc_val_to_string(val: &ScVal) -> Result<String> {
    match val {
        ScVal::Symbol(sym) => Ok(sym.0.to_string()),
        ScVal::String(s) => Ok(s.0.to_string()),
        other => bail!("expected Symbol or String ScVal, got {other:?}"),
    }
}

/// Decodes a `BytesN<32>` (or any `Bytes`) ScVal to a hex string.
pub fn sc_val_to_bytes_hex(val: &ScVal) -> Result<String> {
    match val {
        ScVal::Bytes(bytes) => Ok(hex::encode(bytes.0.as_slice())),
        other => bail!("expected Bytes ScVal, got {other:?}"),
    }
}

/// Converts a contract ledger timestamp (Unix seconds) to a `DateTime`.
/// Contract-side "unset" sentinels (`0`, or an empty string for the
/// URI/title fields these usually accompany) are the caller's concern --
/// this just does the timestamp conversion.
pub fn unix_seconds_to_datetime(secs: u64) -> Result<DateTime<Utc>> {
    DateTime::from_timestamp(secs as i64, 0)
        .ok_or_else(|| anyhow!("timestamp out of range: {secs}"))
}

/// Contract-side optional strings use `""` as "not set" (see the contract's
/// `Milestone::evidence_uri` / `Escrow::title` docs) since Soroban structs
/// don't support `Option` at low friction. The backend has no such
/// constraint, so this is where that sentinel becomes a real `Option`.
pub fn empty_as_none(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Looks up a field by name in a contractevent's data map (keys are always
/// `ScVal::Symbol`s matching the struct's field names).
pub fn map_get<'a>(val: &'a ScVal, field: &str) -> Result<&'a ScVal> {
    let ScVal::Map(Some(map)) = val else {
        bail!("expected a Map ScVal for event data, got {val:?}");
    };
    map.0
        .iter()
        .find(|entry| matches!(&entry.key, ScVal::Symbol(s) if s.0.to_string() == field))
        .map(|entry| &entry.val)
        .ok_or_else(|| anyhow!("missing field '{field}' in event data map"))
}

/// Decodes our `Resolution` contract enum, encoded by `#[contracttype]` as a
/// `Vec` whose first element is a `Symbol` naming the variant, followed by
/// any tuple payload. Returns `(kind, provider_bps)` where `kind` is one of
/// "release", "refund", "split".
pub fn decode_resolution(val: &ScVal) -> Result<(&'static str, Option<i32>)> {
    let ScVal::Vec(Some(vec)) = val else {
        bail!("expected a Vec ScVal for Resolution, got {val:?}");
    };
    let variant = vec
        .0
        .first()
        .ok_or_else(|| anyhow!("empty Resolution vec"))?;
    let ScVal::Symbol(sym) = variant else {
        bail!("expected Symbol as first element of Resolution vec");
    };
    match sym.0.to_string().as_str() {
        "ReleaseToProvider" => Ok(("release", None)),
        "RefundToClient" => Ok(("refund", None)),
        "Split" => {
            let bps_val = vec
                .0
                .get(1)
                .ok_or_else(|| anyhow!("Split resolution missing bps payload"))?;
            let bps = sc_val_to_u32(bps_val)?;
            Ok(("split", Some(bps as i32)))
        }
        other => bail!("unknown Resolution variant: {other}"),
    }
}

pub struct DecodedMilestone {
    pub id: u32,
    pub description: String,
    pub amount: i128,
    pub deadline: u64,
}

/// Decodes the `milestones` field of an `EscrowCreated` event: a `Vec` of
/// `Milestone` structs, each encoded as a `Map` keyed by field name. Only
/// pulls the fields meaningful at creation time -- `status`,
/// `submitted_at`, and the evidence fields are always their "just created"
/// defaults here and arrive for real later via `milestone_submitted`.
pub fn decode_milestones(val: &ScVal) -> Result<Vec<DecodedMilestone>> {
    let ScVal::Vec(Some(vec)) = val else {
        bail!("expected a Vec ScVal for milestones, got {val:?}");
    };
    vec.0
        .iter()
        .map(|entry| {
            Ok(DecodedMilestone {
                id: sc_val_to_u32(map_get(entry, "id")?)?,
                description: sc_val_to_string(map_get(entry, "description")?)?,
                amount: sc_val_to_i128(map_get(entry, "amount")?)?,
                deadline: sc_val_to_u64(map_get(entry, "deadline")?)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use stellar_xdr::{
        AccountId, ContractId, Hash, Int128Parts, ScMap, ScMapEntry, ScString, ScSymbol, ScVec,
        StringM, Uint256, VecM,
    };

    fn symbol(s: &str) -> ScVal {
        ScVal::Symbol(ScSymbol(s.try_into().unwrap()))
    }

    #[test]
    fn decodes_i128_from_parts() {
        let val = ScVal::I128(Int128Parts { hi: 0, lo: 400 });
        assert_eq!(sc_val_to_i128(&val).unwrap(), 400);
    }

    #[test]
    fn decodes_negative_i128_from_parts() {
        let val = ScVal::I128(Int128Parts {
            hi: -1,
            lo: u64::MAX,
        });
        assert_eq!(sc_val_to_i128(&val).unwrap(), -1);
    }

    #[test]
    fn decodes_account_address() {
        let raw = [7u8; 32];
        let addr = ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(raw))));
        let s = sc_address_to_string(&addr).unwrap();
        assert!(s.starts_with('G'));
        let round_tripped = stellar_strkey::ed25519::PublicKey::from_string(&s).unwrap();
        assert_eq!(round_tripped.0, raw);
    }

    #[test]
    fn decodes_contract_address() {
        let raw = [9u8; 32];
        let addr = ScAddress::Contract(ContractId(Hash(raw)));
        let s = sc_address_to_string(&addr).unwrap();
        assert!(s.starts_with('C'));
    }

    #[test]
    fn finds_field_in_data_map() {
        let map = ScVal::Map(Some(ScMap(
            vec![
                ScMapEntry {
                    key: symbol("milestone_id"),
                    val: ScVal::U32(2),
                },
                ScMapEntry {
                    key: symbol("amount"),
                    val: ScVal::I128(Int128Parts { hi: 0, lo: 100 }),
                },
            ]
            .try_into()
            .unwrap(),
        )));

        assert_eq!(
            sc_val_to_u32(map_get(&map, "milestone_id").unwrap()).unwrap(),
            2
        );
        assert_eq!(
            sc_val_to_i128(map_get(&map, "amount").unwrap()).unwrap(),
            100
        );
        assert!(map_get(&map, "missing").is_err());
    }

    #[test]
    fn decodes_release_to_provider_resolution() {
        let val = ScVal::Vec(Some(ScVec(
            VecM::try_from(vec![symbol("ReleaseToProvider")]).unwrap(),
        )));
        let (kind, bps) = decode_resolution(&val).unwrap();
        assert_eq!(kind, "release");
        assert_eq!(bps, None);
    }

    #[test]
    fn decodes_split_resolution() {
        let val = ScVal::Vec(Some(ScVec(
            VecM::try_from(vec![symbol("Split"), ScVal::U32(6_000)]).unwrap(),
        )));
        let (kind, bps) = decode_resolution(&val).unwrap();
        assert_eq!(kind, "split");
        assert_eq!(bps, Some(6_000));
    }

    #[test]
    fn decodes_milestones_vec() {
        let milestone_map = |id: u32, desc: &str, amount: i64| {
            ScVal::Map(Some(ScMap(
                VecM::try_from(vec![
                    ScMapEntry {
                        key: symbol("id"),
                        val: ScVal::U32(id),
                    },
                    ScMapEntry {
                        key: symbol("description"),
                        val: ScVal::String(ScString(desc.try_into().unwrap())),
                    },
                    ScMapEntry {
                        key: symbol("amount"),
                        val: ScVal::I128(Int128Parts {
                            hi: 0,
                            lo: amount as u64,
                        }),
                    },
                    ScMapEntry {
                        key: symbol("status"),
                        val: ScVal::Vec(Some(ScVec(
                            VecM::try_from(vec![symbol("Pending")]).unwrap(),
                        ))),
                    },
                    ScMapEntry {
                        key: symbol("deadline"),
                        val: ScVal::U64(1_700_000_000),
                    },
                ])
                .unwrap(),
            )))
        };

        let val = ScVal::Vec(Some(ScVec(
            VecM::try_from(vec![
                milestone_map(0, "Design", 100),
                milestone_map(1, "Build", 300),
            ])
            .unwrap(),
        )));

        let milestones = decode_milestones(&val).unwrap();
        assert_eq!(milestones.len(), 2);
        assert_eq!(milestones[0].id, 0);
        assert_eq!(milestones[0].description, "Design");
        assert_eq!(milestones[0].amount, 100);
        assert_eq!(milestones[0].deadline, 1_700_000_000);
        assert_eq!(milestones[1].amount, 300);
    }

    #[test]
    fn decodes_bytes_to_hex() {
        let val = ScVal::Bytes(stellar_xdr::ScBytes(
            vec![0xdeu8, 0xad, 0xbe, 0xef].try_into().unwrap(),
        ));
        assert_eq!(sc_val_to_bytes_hex(&val).unwrap(), "deadbeef");
    }

    #[test]
    fn converts_unix_seconds_to_datetime() {
        let dt = unix_seconds_to_datetime(1_700_000_000).unwrap();
        assert_eq!(dt.timestamp(), 1_700_000_000);
    }

    #[test]
    fn empty_string_becomes_none() {
        assert_eq!(empty_as_none(String::new()), None);
        assert_eq!(
            empty_as_none("ipfs://x".to_string()),
            Some("ipfs://x".to_string())
        );
    }

    #[test]
    fn decodes_symbol_string() {
        let sym: StringM<32> = "escrow_created".try_into().unwrap();
        assert_eq!(
            sc_val_to_string(&ScVal::Symbol(ScSymbol(sym))).unwrap(),
            "escrow_created"
        );
    }
}
