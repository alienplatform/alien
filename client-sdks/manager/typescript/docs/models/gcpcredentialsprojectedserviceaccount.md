# GcpCredentialsProjectedServiceAccount

Use projected service account token (for Kubernetes workload identity)

## Example Usage

```typescript
import { GcpCredentialsProjectedServiceAccount } from "@alienplatform/manager-api/models";

let value: GcpCredentialsProjectedServiceAccount = {
  serviceAccountEmail: "<value>",
  tokenFile: "<value>",
  type: "projectedServiceAccount",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `serviceAccountEmail`                       | *string*                                    | :heavy_check_mark:                          | Service account email                       |
| `tokenFile`                                 | *string*                                    | :heavy_check_mark:                          | Path to the projected service account token |
| `type`                                      | *"projectedServiceAccount"*                 | :heavy_check_mark:                          | N/A                                         |