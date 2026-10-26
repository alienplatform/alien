
## Shared Azure Blob settings

The generated storage account is shared by this deployment's containers. Setup owns its settings, including settings requested by containers created later by the runtime. Apply changes through the setup update workflow above.

- Versioning applies to the whole account when any enabled container requests it. This also versions objects in sibling containers and can increase storage costs.
- CORS combines the browser origins requested by enabled containers into one account-wide GET/HEAD rule. Those origins apply to every container in the account. CORS does not grant object access: private objects still require authorization, such as a signed URL. The generated plan checks Azure's combined CORS size limits.
- Public-access capability is enabled only when an enabled container requests public reads. Each container still has its own public/private setting; private siblings remain private.
