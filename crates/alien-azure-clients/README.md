# alien-azure-clients

Custom HTTP client for Azure APIs. Makes direct API calls using `reqwest` with Azure token authentication. Model types are code-generated from Azure OpenAPI specs via Progenitor at build time.

Services: Container Apps, Blob Containers, Service Bus, Key Vault, Managed Identity, Authorization, Container Registry, VMSS, Load Balancers, Network, Storage Accounts, Table Storage, Managed Disks, Resources.

Trait-based API design with `mockall` support.
