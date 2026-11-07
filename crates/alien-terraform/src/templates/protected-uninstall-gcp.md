
## Uninstall a protected GCP setup

The verifier records each release it accepts as a new version of the Parameter Manager parameter `signed-<prefix>-state`. Terraform created only its first version, `v1`, and GCP refuses to delete a parameter that still has versions, so `terraform destroy` stops on that parameter. Delete the verifier's versions first, then destroy:

```bash
PARAMETER="signed-$(terraform output -raw deployment_resource_prefix)-state"
gcloud parametermanager parameters versions list --parameter="$PARAMETER" --location=global \
  --format='value(name.basename())' | grep -vx v1 \
  | xargs -r -I{} gcloud parametermanager parameters versions delete {} \
      --parameter="$PARAMETER" --location=global --quiet
terraform destroy
```

Run it with an identity allowed `parametermanager.parameterVersions.delete`, such as the one that ran setup. Destroying the setup removes the install's protection with it.
