# GCP Observe Read Role

Grants read-only access that Alien uses to discover and observe existing GCP resources in one project.
Apply this in the project you want to observe.

## Terraform

```hcl
module "alien_observe_read_role" {
  source = "github.com/aliendotdev/alien//infra/gcp/observe-read-role"

  project_id = "my-gcp-project"
  member     = "serviceAccount:alien-manager@my-gcp-project.iam.gserviceaccount.com"
}

output "alien_observe_roles" {
  value = module.alien_observe_read_role.roles
}
```

## gcloud

```bash
gcloud projects add-iam-policy-binding my-gcp-project \
  --member="serviceAccount:alien-manager@my-gcp-project.iam.gserviceaccount.com" \
  --role="roles/cloudasset.viewer"

gcloud projects add-iam-policy-binding my-gcp-project \
  --member="serviceAccount:alien-manager@my-gcp-project.iam.gserviceaccount.com" \
  --role="roles/monitoring.viewer"
```

`roles/cloudasset.viewer` allows project inventory discovery through Cloud Asset Inventory.
`roles/monitoring.viewer` allows partial health and metrics reads through Cloud Monitoring.
