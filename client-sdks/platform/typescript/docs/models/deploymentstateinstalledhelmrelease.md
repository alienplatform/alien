# DeploymentStateInstalledHelmRelease

The Helm release an Operator reports it was installed as, so the exact
upgrade command can be shown. Reported, never trusted for authority.

## Example Usage

```typescript
import { DeploymentStateInstalledHelmRelease } from "@alienplatform/platform-api/models";

let value: DeploymentStateInstalledHelmRelease = {
  name: "<value>",
  namespace: "<value>",
};
```

## Fields

| Field                                             | Type                                              | Required                                          | Description                                       |
| ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- |
| `name`                                            | *string*                                          | :heavy_check_mark:                                | Release name, as passed to `helm install`.        |
| `namespace`                                       | *string*                                          | :heavy_check_mark:                                | Kubernetes namespace the release is installed in. |