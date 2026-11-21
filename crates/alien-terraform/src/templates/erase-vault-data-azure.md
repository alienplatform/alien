

## Erase vault values after an erase delete

Values the application wrote to a vault are secrets in the Key Vault of that vault, and the delete does not remove them. If you chose to erase data, run `erase-vault-data.py` after the delete finishes and before `terraform destroy`, with your own Azure credentials (`az` on PATH, signed in). It reads the Key Vaults from this module's Terraform state, so it only touches Key Vaults this module created, never a vault binding you supplied yourself. Use the same Terraform backend, workspace and working directory as for the install; for a child module add `--module-address 'module.<name>'`.

You need Key Vault Secrets Officer, or an equivalent role with data-plane delete and purge permissions, on each of those Key Vaults, and network access to them.

```bash
# Dry run: lists the secrets of each Key Vault by exact name and deletes nothing.
python3 ./erase-vault-data.py --receipt-dir /secure/path/erase-vault
# Delete exactly what the preview listed. The helper lists again first and stops if anything changed.
python3 ./erase-vault-data.py --receipt-dir /secure/path/erase-vault --confirm
```

Each vault of this module has its own Key Vault, so everything in it belongs to this install. The preview has two groups, and each says which flag deletes it:

- **vault values**: every secret in this module's own Key Vaults. `--confirm` deletes them.
- **deployer secrets**: every secret in this module's deployer secrets Key Vault. `--include-deployer-secrets` deletes them, together with `--confirm`.

For each Key Vault the helper reads the live purge protection setting and soft-delete retention. With purge protection off it purges each deleted secret right after deleting it. This module always turns purge protection on, and Key Vault does not allow purging while it is on, so the helper does not try: the deleted values stay recoverable in the Key Vault's soft-delete state until about the deletion time plus the retention days. The preview and the end of the run print that date per Key Vault, and the receipt records it as `purge_blocked_until`. The helper never deletes or purges a Key Vault itself: it stays retained setup data under the "Retain Azure setup data during uninstall" section above, with its own recovery window.

`--confirm` writes a receipt in `--receipt-dir` with each deleted secret, its Key Vault, its id, its group, whether it was purged, the flags, the time and the subscription and account. It refuses to run without a preview of the same set in that directory. Values written before this module version carry no marker, but the Key Vault is dedicated, so they are covered like any other.
