//! Reads an Anchor IDL (the modern format from Anchor 0.30 onward, or the
//! legacy format used before it) into one common shape.

use crate::json::{self, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Anchor 0.30 and later: `writable`, `signer`, `address`, `pda`, `relations`.
    Modern,
    /// Before Anchor 0.30: `isMut`, `isSigner`, optional `pda` and `relations`.
    Legacy,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Modern => "Anchor 0.30+ IDL",
            Format::Legacy => "legacy IDL (before Anchor 0.30)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Program {
    pub name: String,
    pub format: Format,
    pub instructions: Vec<Instruction>,
}

#[derive(Debug, Clone)]
pub struct Instruction {
    /// Name as written in the IDL.
    pub name: String,
    /// Name used for matching across versions (see `match_key`).
    pub key: String,
    pub discriminator: Option<Vec<u8>>,
    /// Accounts in the order clients must pass them, nested groups flattened.
    pub accounts: Vec<Account>,
    pub args: Vec<Arg>,
}

#[derive(Debug, Clone)]
pub struct Account {
    /// Display path, e.g. `pool_state` or `group.authority` for nested groups.
    pub name: String,
    pub key: String,
    pub writable: bool,
    pub signer: bool,
    pub optional: bool,
    pub address: Option<String>,
    /// PDA seeds written as one canonical string, if the IDL declares them.
    pub pda: Option<String>,
    /// `has_one` style relations, as match keys, sorted.
    pub relations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Arg {
    pub name: String,
    pub key: String,
    /// Canonical type string, comparable across IDL formats.
    pub ty: String,
}

/// Names are compared with case and underscores removed, so `ammConfig`
/// (legacy camelCase), `amm_config` and `token0Vault` / `token_0_vault`
/// all line up across IDL formats.
pub fn match_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[derive(Debug)]
pub struct IdlError(pub String);

impl std::fmt::Display for IdlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn load(text: &str) -> Result<Program, IdlError> {
    let root = json::parse(text).map_err(|e| IdlError(format!("not valid JSON: {e}")))?;
    from_value(&root)
}

pub fn from_value(root: &Value) -> Result<Program, IdlError> {
    if !matches!(root, Value::Object(_)) {
        return Err(IdlError(
            "not an Anchor IDL: the top level must be a JSON object".into(),
        ));
    }
    let instructions = root
        .get("instructions")
        .and_then(Value::as_array)
        .ok_or_else(|| IdlError("not an Anchor IDL: no \"instructions\" array".into()))?;

    // Modern IDLs carry their spec in metadata; legacy ones have top-level name/version.
    let modern = root.get("metadata").and_then(|m| m.get("spec")).is_some()
        || instructions
            .iter()
            .any(|ix| ix.get("discriminator").is_some());
    let format = if modern {
        Format::Modern
    } else {
        Format::Legacy
    };

    let name = root
        .get("metadata")
        .and_then(|m| m.get("name"))
        .or_else(|| root.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    let mut out = Vec::new();
    for ix in instructions {
        out.push(instruction(ix, format)?);
    }
    Ok(Program {
        name,
        format,
        instructions: out,
    })
}

fn instruction(ix: &Value, format: Format) -> Result<Instruction, IdlError> {
    let name = ix
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| IdlError("an instruction has no name".into()))?
        .to_string();
    let discriminator = ix
        .get("discriminator")
        .and_then(Value::as_array)
        .map(|bytes| bytes.iter().filter_map(Value::as_u8).collect());

    let mut accounts = Vec::new();
    if let Some(list) = ix.get("accounts").and_then(Value::as_array) {
        flatten(list, "", format, &mut accounts, &name)?;
    }

    let mut args = Vec::new();
    for a in ix.get("args").and_then(Value::as_array).unwrap_or(&[]) {
        let arg_name = a
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let ty = a
            .get("type")
            .map(canonical_type)
            .unwrap_or_else(|| "?".into());
        args.push(Arg {
            key: match_key(&arg_name),
            name: arg_name,
            ty,
        });
    }

    Ok(Instruction {
        key: match_key(&name),
        name,
        discriminator,
        accounts,
        args,
    })
}

/// Nested account groups (`#[derive(Accounts)]` structs used inside another)
/// are flattened in order, with the group name as a prefix.
fn flatten(
    list: &[Value],
    prefix: &str,
    format: Format,
    out: &mut Vec<Account>,
    ix: &str,
) -> Result<(), IdlError> {
    for a in list {
        let raw = a
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| IdlError(format!("an account in instruction `{ix}` has no name")))?;
        let name = if prefix.is_empty() {
            raw.to_string()
        } else {
            format!("{prefix}.{raw}")
        };

        if let Some(children) = a.get("accounts").and_then(Value::as_array) {
            flatten(children, &name, format, out, ix)?;
            continue;
        }

        let flag = |modern: &str, legacy: &str| {
            let key = if format == Format::Modern {
                modern
            } else {
                legacy
            };
            a.get(key).and_then(Value::as_bool).unwrap_or(false)
        };

        let mut relations: Vec<String> = a
            .get("relations")
            .and_then(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .filter_map(Value::as_str)
            .map(match_key)
            .collect();
        relations.sort();
        relations.dedup();

        out.push(Account {
            key: name.split('.').map(match_key).collect::<Vec<_>>().join("."),
            name,
            writable: flag("writable", "isMut"),
            signer: flag("signer", "isSigner"),
            optional: flag("optional", "isOptional"),
            address: a.get("address").and_then(Value::as_str).map(str::to_string),
            pda: a.get("pda").map(canonical_pda),
            relations,
        });
    }
    Ok(())
}

/// A PDA written as `seeds=[...] program=...`, with names normalised so a
/// cosmetic rename does not count as a change.
fn canonical_pda(pda: &Value) -> String {
    let seeds: Vec<String> = pda
        .get("seeds")
        .and_then(Value::as_array)
        .unwrap_or(&[])
        .iter()
        .map(canonical_seed)
        .collect();
    let mut s = format!("seeds=[{}]", seeds.join(", "));
    if let Some(p) = pda.get("program") {
        s.push_str(&format!(" program={}", canonical_seed(p)));
    }
    s
}

fn canonical_seed(seed: &Value) -> String {
    let kind = seed.get("kind").and_then(Value::as_str).unwrap_or("?");
    match kind {
        "const" => match seed.get("value") {
            // Modern IDLs store constants as byte arrays; show them as text when readable.
            Some(Value::Array(bytes)) => {
                let raw: Vec<u8> = bytes.iter().filter_map(Value::as_u8).collect();
                match std::str::from_utf8(&raw) {
                    Ok(s) if !s.is_empty() && s.chars().all(|c| c.is_ascii_graphic()) => {
                        format!("\"{s}\"")
                    }
                    _ => format!(
                        "0x{}",
                        raw.iter().map(|b| format!("{b:02x}")).collect::<String>()
                    ),
                }
            }
            Some(v) => v.to_string(),
            None => "const(?)".into(),
        },
        "arg" | "account" => {
            let path = seed.get("path").and_then(Value::as_str).unwrap_or("?");
            let path = path.split('.').map(match_key).collect::<Vec<_>>().join(".");
            match seed.get("account").and_then(Value::as_str) {
                Some(acc) if kind == "account" => format!("account:{path}({})", match_key(acc)),
                _ => format!("{kind}:{path}"),
            }
        }
        other => format!("{other}:{seed}"),
    }
}

/// Type strings that compare equal across the legacy and modern formats
/// (`publicKey` vs `pubkey`, `{"defined":"X"}` vs `{"defined":{"name":"X"}}`).
pub fn canonical_type(ty: &Value) -> String {
    match ty {
        Value::String(s) => match s.as_str() {
            "publicKey" => "pubkey".into(),
            other => other.into(),
        },
        Value::Object(fields) if fields.len() == 1 => {
            let (k, v) = &fields[0];
            match k.as_str() {
                "defined" => {
                    let name = match v {
                        Value::String(s) => s.clone(),
                        _ => v
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("?")
                            .to_string(),
                    };
                    let generics = v
                        .get("generics")
                        .map(|g| format!("<{g}>"))
                        .unwrap_or_default();
                    format!("{name}{generics}")
                }
                "option" | "vec" | "coption" => format!("{k}<{}>", canonical_type(v)),
                "array" => match v.as_array() {
                    Some([inner, len]) => format!("[{}; {}]", canonical_type(inner), len),
                    _ => format!("array{v}"),
                },
                _ => format!("{k}:{v}"),
            }
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_keys_line_up_across_formats() {
        assert_eq!(match_key("ammConfig"), match_key("amm_config"));
        assert_eq!(match_key("token0Vault"), match_key("token_0_vault"));
        assert_ne!(match_key("owner"), match_key("authority"));
    }

    #[test]
    fn types_line_up_across_formats() {
        let legacy = json::parse(r#"{"defined":"SwapParams"}"#).unwrap();
        let modern = json::parse(r#"{"defined":{"name":"SwapParams"}}"#).unwrap();
        assert_eq!(canonical_type(&legacy), canonical_type(&modern));
        assert_eq!(canonical_type(&Value::String("publicKey".into())), "pubkey");
        let arr = json::parse(r#"{"array":["u8",32]}"#).unwrap();
        assert_eq!(canonical_type(&arr), "[u8; 32]");
        let opt = json::parse(r#"{"option":{"vec":"u64"}}"#).unwrap();
        assert_eq!(canonical_type(&opt), "option<vec<u64>>");
    }

    #[test]
    fn reads_modern_accounts() {
        let p = load(r#"{"address":"x","metadata":{"name":"p","version":"0.1.0","spec":"0.1.0"},
            "instructions":[{"name":"swap","discriminator":[1,2,3,4,5,6,7,8],
              "accounts":[
                {"name":"payer","signer":true},
                {"name":"pool","writable":true,"relations":["admin"],
                 "pda":{"seeds":[{"kind":"const","value":[112,111,111,108]},{"kind":"account","path":"mint"}]}},
                {"name":"group","accounts":[{"name":"inner","address":"Abc"}]}],
              "args":[{"name":"amount_in","type":"u64"}]}]}"#)
        .unwrap();
        assert_eq!(p.format, Format::Modern);
        assert_eq!(p.name, "p");
        let ix = &p.instructions[0];
        assert_eq!(
            ix.discriminator.as_deref(),
            Some(&[1, 2, 3, 4, 5, 6, 7, 8][..])
        );
        assert!(ix.accounts[0].signer && !ix.accounts[0].writable);
        assert_eq!(
            ix.accounts[1].pda.as_deref(),
            Some("seeds=[\"pool\", account:mint]")
        );
        assert_eq!(ix.accounts[1].relations, vec!["admin"]);
        assert_eq!(ix.accounts[2].name, "group.inner");
        assert_eq!(ix.accounts[2].address.as_deref(), Some("Abc"));
        assert_eq!(ix.args[0].ty, "u64");
    }

    #[test]
    fn reads_legacy_accounts() {
        let p = load(r#"{"version":"0.1.0","name":"p","instructions":[{"name":"createPool",
              "accounts":[{"name":"payer","isMut":true,"isSigner":true},{"name":"maybe","isMut":false,"isSigner":false,"isOptional":true}],
              "args":[{"name":"owner","type":"publicKey"}]}]}"#)
        .unwrap();
        assert_eq!(p.format, Format::Legacy);
        let ix = &p.instructions[0];
        assert_eq!(ix.key, "createpool");
        assert!(ix.accounts[0].signer && ix.accounts[0].writable);
        assert!(ix.accounts[1].optional);
        assert_eq!(ix.args[0].ty, "pubkey");
    }

    #[test]
    fn rejects_non_idl_json() {
        assert!(load("[1,2]").is_err());
        assert!(load(r#"{"name":"x"}"#).is_err());
        assert!(load("not json").is_err());
    }
}
