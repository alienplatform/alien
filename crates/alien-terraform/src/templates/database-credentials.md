
## Database credentials

This module requires Terraform 1.11 or newer. Database passwords use ephemeral
values and provider write-only arguments. Only secret identifiers, versions and
rotation revisions enter the current Terraform state and saved plan.

Each database has a `<terraform-label>_password_revision` input, initially `1`.
Increment it explicitly to rotate that password. Apply the secret and database
changes together. If an apply fails after writing the secret, retry with the
**same revision**: the database reads the exact stored secret version instead of
generating a different password. Do not independently rotate these setup-owned
secret versions; use the revision input so the database and secret stay aligned.
The setup identity needs permission to read this database secret as well as
write it. Runtime management does not gain secret-read access.

Updating an older package rotates its password in place; review the plan for
credential updates and preserved database identity before applying it. Previously
saved state versions and plan files can still contain the old password. After
successful rotation and an authenticated database check, handle those historical
files under your state-backend access and retention policy. This module does not
rewrite state history or delete those backups. A database reinstall is unnecessary.
