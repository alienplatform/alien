

## Erase vault values after an erase delete

Values the application wrote to a vault are secrets in the Key Vault of that vault, and the delete does not remove them. If you chose to erase data, run `erase-vault-data.py` after the delete finishes and before `terraform destroy`, with your own Azure credentials (`az` on PATH, signed in). It reads the Key Vaults from this module's Terraform state, so it only touches Key Vaults this module created, never a vault binding you supplied yourself. Use the same Terraform backend, workspace and working directory as for the install; for a child module add `--module-address 'module.<name>'`. Before it lists anything, the helper checks that the signed-in subscription is the one of this module's Key Vaults (read from the Terraform state) and refuses, with nothing deleted, when it differs.

You need Key Vault Secrets Officer, or an equivalent role with data-plane delete and purge permissions, on each of those Key Vaults, and network access to them.

```bash
# Dry run: lists the secrets of each Key Vault by exact name and deletes nothing.
python3 ./erase-vault-data.py --receipt-dir /secure/path/erase-vault
# Delete exactly what the preview listed. The helper lists again first and stops if anything changed.
python3 ./erase-vault-data.py --receipt-dir /secure/path/erase-vault --confirm
```

Each vault of this module has its own Key Vault. The vault provider tags every secret it writes with the Key Vault name (`alien-vault-prefix`) and the vault id (`alien-vault`); Key Vault tags belong to a secret version, so the tags go on every write, which is safe because nothing else writes to a dedicated Key Vault. The preview has three groups, and each says which flag deletes it:

- **marked**: secrets whose tags name this module's Key Vault and vault id exactly. `--confirm` deletes them.
- **unmarked**: secrets in this module's vaults without a marker, such as values written before this module version or by the stack's own setup and management identities. `--include-unmarked` deletes them, together with `--confirm`. A secret marked for another vault of this module is left alone and never deleted.
- **deployer secrets**: every secret in this module's deployer secrets Key Vault. `--include-deployer-secrets` deletes them, together with `--confirm`.

The helper finds each Key Vault by its exact Terraform address, written into `erase-vault-data.json` when the module was generated.

For each Key Vault the helper reads the live purge protection setting and soft-delete retention. With purge protection off it purges each deleted secret right after deleting it. This module always turns purge protection on, and Key Vault does not allow purging while it is on, so the helper does not try: the deleted values stay recoverable in the Key Vault's soft-delete state until about the deletion time plus the retention days. Until then the same secret name cannot be created again, so a deployment recreated with the same names fails to write them unless you recover the deleted secrets (`az keyvault secret recover`) or wait for that date. The preview warns when the Terraform state still has the Operator Helm release (the chart is still installed and values may still change; delete the deployment and wait first). The preview and the end of the run print that date per Key Vault, and the receipt records it as `purge_blocked_until`. The helper never deletes or purges a Key Vault itself: it stays retained setup data under the "Retain Azure setup data during uninstall" section above, with its own recovery window.

`--confirm` writes a receipt in `--receipt-dir` with each deleted secret, its Key Vault, its id, its group, whether it was purged, the flags, the time and the subscription and account. It refuses to run without a preview of the same set in that directory.
