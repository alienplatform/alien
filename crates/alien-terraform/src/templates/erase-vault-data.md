

## Erase vault values after an erase delete

Values the application wrote to a vault bound to {{STORE}} are not in Terraform state, and the delete does not remove them. If you chose to erase data, run `erase-vault-data.py` after the delete finishes and before `terraform destroy`, with your own {{CLOUD}} credentials ({{CLI}} on PATH). It looks only at the vaults this module set up, never at a vault binding you supplied yourself, and it needs no new access for the Operator or the application. Use the same Terraform backend, workspace and working directory as for the install; for a child module add `--module-address 'module.<name>'`.

```bash
# Dry run: lists the values by exact name in three groups and deletes nothing.
python3 ./erase-vault-data.py {{SCOPE}} --receipt-dir /secure/path/erase-vault
# Delete exactly what the preview listed as marked. The helper lists again first and stops if anything changed.
python3 ./erase-vault-data.py {{SCOPE}} --receipt-dir /secure/path/erase-vault --confirm
```

The preview has three groups, and each says which flag deletes it:

- **marked**: values whose vault recorded this module's vault prefix and vault id when it created them. `--confirm` deletes them.
- **unmarked**: values with a vault prefix of this module in their name and no marker. `--include-unmarked` deletes them, together with `--confirm`.
- **deployer secrets**: values under the `<resource prefix>-deployer-secrets-` prefix. `--include-deployer-secrets` deletes them, together with `--confirm`.

Values written before this module version carry no marker, so they appear as unmarked. A vault prefix is the resource prefix, a hyphen and the vault id, and the value name follows it with no separator, so one vault's prefix can also match another vault's values (`secrets` and `secrets-x`). The unmarked group leaves out any name that also starts with another vault prefix of this module; those names are listed as left alone and are never deleted. Values that the stack's own setup and management identities create, such as the egress signing key or the gateway config, are never marked and also appear as unmarked. Read the unmarked names before you add `--include-unmarked`. If you set `resource_prefix` yourself, the helper asks you to confirm that no other install uses it: another install with the same prefix writes values with the same names and the same markers, so both the marked and unmarked groups can include its values. The preview warns you, and `--confirm` refuses until you also pass `--shared-prefix-reviewed`. With the generated prefix that flag is not needed.

`--confirm` writes a receipt in `--receipt-dir` with each deleted name, its cloud id, its group, the flags, the time and the {{ACCOUNT}}. It refuses to run without a preview of the same set in that directory. The receipt also records `--shared-prefix-reviewed`.
