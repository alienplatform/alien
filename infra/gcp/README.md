# GCP Module

Provisions the GCP resources that alien-manager needs for push-mode deployments. Does **not** deploy compute — run the manager wherever you like and point it at these resources.

## Resources Created

- **Service Account**: IAM identity for the manager (attach to your compute)
- **Artifact Registry** (optional): Docker repository
- **Commands Store** (optional): Firestore Native database and GCS bucket
- **Impersonation** (optional): Service account for cross-project impersonation

## Usage

```hcl
module "alien_infra" {
  source = "github.com/aliendotdev/alien//infra/gcp"

  name       = "my-project"
  project_id = "my-gcp-project"
  region     = "us-central1"

  enable_artifact_registry = true
  enable_commands_store    = true
  enable_impersonation     = true

  labels = {
    environment = "production"
  }
}
```

Use `config_values` to populate your `alien-manager.toml`:

```hcl
output "toml_sections" {
  value = module.alien_infra.config_values
}
```

Attach the service account to your compute:

```hcl
output "manager_service_account" {
  value = module.alien_infra.service_account_email
}
```

## Variables

| Name | Description | Type | Default | Required |
|------|-------------|------|---------|----------|
| `name` | Name prefix for all resources | `string` | — | yes |
| `project_id` | GCP project ID | `string` | — | yes |
| `region` | GCP region | `string` | — | yes |
| `enable_artifact_registry` | Create Artifact Registry repository | `bool` | `true` | no |
| `enable_commands_store` | Create Firestore + GCS for commands | `bool` | `false` | no |
| `enable_impersonation` | Create service account for impersonation | `bool` | `false` | no |
| `impersonation_members` | IAM members for impersonation trust | `list(string)` | `[]` | no |
| `labels` | Labels for all resources | `map(string)` | `{}` | no |

## Outputs

| Name | Description |
|------|-------------|
| `config_values` | Structured values for `alien-manager.toml` sections |
| `service_account_email` | Manager service account email (attach to your compute) |
| `gar_repository_name` | Artifact Registry repository name (if enabled) |
| `gar_repository_url` | Artifact Registry repository URL (if enabled) |
