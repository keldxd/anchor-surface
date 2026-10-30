# anchor-surface

Compare two versions of an Anchor program's IDL and see every change to **who can call what**: signers, writable accounts, fixed addresses, PDA seeds, `has_one` relations, new instructions, and changes that break existing clients.

Run it before you upgrade a Solana program, or in CI on every pull request.

```
$ anchor-surface old/vault.json new/vault.json

DANGER  initialize / admin  [address-removed]
        was fixed to Adm1n1111111111111111111111111111111111111; any signer can now act here unless the program checks the key in code
DANGER  withdraw / owner  [signer-removed]
        no longer required to sign; any account can be passed here without its signature unless the program checks it in code
DANGER  withdraw / position  [relation-removed]
        relation `owner` removed (has_one check); this account is no longer tied to `owner`
REVIEW  emergency_sweep  [instruction-added]
        new instruction; no account must sign, so anyone can call it; writes to 3 account(s)
...
5 danger, 2 review, 0 info
```

Status: **v0.1, early.** Tested on the fixtures in this repo and on Raydium's published IDL history. See [Limits](#limits) before relying on it.

## Why

In Anchor, a lot of access control lives in the account declarations: `Signer`, `has_one`, `address = ...`, `seeds = [...]`. Deleting one of them is a one-line change that compiles, passes most tests and is easy to miss in a large diff. The IDL that `anchor build` generates records these declarations, so comparing the old and new IDL shows exactly which ones changed, in seconds, without reading the whole diff.

## Install

From source (needs a recent stable Rust toolchain):

```
cargo install --git https://github.com/keldxd/anchor-surface
```

anchor-surface has **no dependencies** beyond the Rust standard library, including its JSON reader. Nothing third-party is pulled into your build.

## Usage

```
anchor-surface [OPTIONS] <OLD_IDL.json> <NEW_IDL.json>

  --format <text|json>                  Output format [default: text]
  --fail-on <danger|review|info|never>  Exit with code 1 if any finding is at
                                        or above this severity [default: danger]
```

Exit codes: `0` nothing at or above `--fail-on`, `1` at least one such finding, `2` usage error or a file that is not an Anchor IDL.

Getting the two IDLs:

- **New version:** `anchor build` writes it to `target/idl/<program>.json`.
- **Old version:** check out the deployed commit and run `anchor build` there, or fetch the IDL published on-chain with the Anchor CLI: `anchor idl fetch <PROGRAM_ID> > old.json`.

Both the current IDL format (Anchor 0.30 and later) and the legacy format are supported. Names are matched across formats, so legacy `ammConfig` lines up with `amm_config`.

### In CI

```yaml
- run: anchor build
- run: anchor idl fetch <PROGRAM_ID> > deployed.json   # the IDL of what is live now
- run: anchor-surface deployed.json target/idl/my_program.json
```

The job fails on any `DANGER` finding by default. Use `--format json` to feed the results into other tools.

## What it checks

| Rule | Severity | Meaning |
|---|---|---|
| `signer-removed` | DANGER | An account no longer has to sign |
| `signer-account-removed` | DANGER | An account that had to sign was removed from the instruction |
| `relation-removed` | DANGER | A `has_one` relation was removed |
| `address-removed` / `address-changed` | DANGER | A fixed address was removed or changed (for example an admin key) |
| `pda-removed` / `pda-changed` | DANGER | A PDA seed check was removed, or the seeds changed so existing accounts no longer match |
| `optional-added` (on a signer) | DANGER | A signing account became optional, so the signature can be skipped |
| `discriminator-changed` | DANGER | Every existing client call will fail |
| `args-changed` | DANGER | Arguments added, removed, retyped or reordered; existing clients encode the wrong data |
| `account-order-changed` | DANGER | Clients that pass accounts by position will pass the wrong ones |
| `instruction-added` | REVIEW | New instruction, with who must sign it (or "anyone can call it") |
| `writable-added` | REVIEW | The instruction can now modify an account it could not before |
| `signer-added`, `optional-added`, `optional-removed`, `account-added`, `account-removed`, `instruction-removed` | REVIEW | Behaviour or client compatibility changed |
| `*-added` checks, `writable-removed` | INFO | A check was added or tightened |

Documentation-only changes are ignored.

## Results on real IDL history

Raydium publishes the IDLs of its programs in [raydium-io/raydium-idl](https://github.com/raydium-io/raydium-idl). [`scripts/raydium-demo.sh`](scripts/raydium-demo.sh) downloads them at pinned commits and runs anchor-surface on consecutive versions. Full outputs are in [`demo/`](demo/).

| Program and versions | Result | What it found |
|---|---|---|
| CPMM, June 2025 to May 2026 | 1 danger, 4 review | 4 new instructions (with their signers), and a new `creator_fee_rate` argument on `create_amm_config` that breaks old clients |
| CLMM, June 2025 to May 2026 | 14 danger, 10 review, 7 info | The fixed admin key changed in 6 instructions, the fixed admin key was removed from `create_support_mint_associated`, the PDA check on `protocol_position` was removed in 7 instructions, and 9 new instructions |
| CLMM, Aug 2024 to Dec 2024 | 1 danger, 1 review | New argument on `open_position_v2`, 1 new instruction |
| CLMM, Dec 2024 to June 2025 | 1 review | 1 new instruction; legacy to current format, so addresses, PDAs and relations were not compared |

Every finding above was checked against the raw IDL files. **These are deliberate changes by Raydium, not vulnerabilities.** A rotated admin key or a retired account looks exactly like this. That is the point: these are the lines a reviewer must confirm on purpose before an upgrade ships, and the tool lists them without anyone reading the whole diff.

[`demo/planted_example_vault.txt`](demo/planted_example_vault.txt) shows the output for [a fixture](tests/fixtures/vault_v2_planted.json) with seven planted changes (five loosened checks, a newly writable account and a new instruction anyone can call). All seven are reported, and the test suite checks this on every commit.

## Limits

- **Only what the IDL records is checked.** Checks written in program code (`require!`, custom `constraint = ...` expressions, manual key comparisons) do not appear in the IDL, so removing one is not reported. No findings does not mean an upgrade is safe.
- **Account data layout is not checked.** Use [layoutd](https://docs.rs/crate/layoutd) for changes to account structs.
- **Legacy IDLs record less.** When one side is in the legacy format (before Anchor 0.30), fixed addresses, PDA seeds and relations are not compared, and the report says so.
- **Native (non-Anchor) programs** usually have no IDL, so they are out of scope.
- **Accounts are matched by name.** A renamed account shows as one removed and one added.

## Related tools

- [idl-drift](https://github.com/auroraxo/idl-drift): checks that the IDL committed in a repo matches the one published on-chain.
- [layoutd](https://docs.rs/crate/layoutd): checks account data layout changes.

anchor-surface compares two versions of the access-control surface. The three fit together.

## License

MIT
