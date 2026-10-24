# HelmRelease

A Helm release, identified by its name and namespace.

## Example Usage

```typescript
import { HelmRelease } from "@alienplatform/manager-api/models";

let value: HelmRelease = {
  name: "<value>",
  namespace: "<value>",
};
```

## Fields

| Field                                             | Type                                              | Required                                          | Description                                       |
| ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- |
| `name`                                            | *string*                                          | :heavy_check_mark:                                | Release name, as passed to `helm install`.        |
| `namespace`                                       | *string*                                          | :heavy_check_mark:                                | Kubernetes namespace the release is installed in. |
