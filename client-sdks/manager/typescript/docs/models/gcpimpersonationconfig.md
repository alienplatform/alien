# GcpImpersonationConfig

Configuration for GCP service account impersonation

## Example Usage

```typescript
import { GcpImpersonationConfig } from "@alienplatform/manager-api/models";

let value: GcpImpersonationConfig = {
  scopes: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  serviceAccountEmail: "<value>",
};
```

## Fields

| Field                                                                                                                                        | Type                                                                                                                                         | Required                                                                                                                                     | Description                                                                                                                                  |
| -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `delegates`                                                                                                                                  | *string*[]                                                                                                                                   | :heavy_minus_sign:                                                                                                                           | Optional sequence of service accounts in a delegation chain                                                                                  |
| `lifetime`                                                                                                                                   | *string*                                                                                                                                     | :heavy_minus_sign:                                                                                                                           | Optional desired lifetime duration of the access token (max 3600s)                                                                           |
| `scopes`                                                                                                                                     | *string*[]                                                                                                                                   | :heavy_check_mark:                                                                                                                           | The OAuth 2.0 scopes that define the access token's permissions                                                                              |
| `serviceAccountEmail`                                                                                                                        | *string*                                                                                                                                     | :heavy_check_mark:                                                                                                                           | The email of the service account to impersonate                                                                                              |
| `targetProjectId`                                                                                                                            | *string*                                                                                                                                     | :heavy_minus_sign:                                                                                                                           | Optional target project ID override. When provided, the impersonated config<br/>uses this project ID instead of inheriting the caller's project. |
| `targetRegion`                                                                                                                               | *string*                                                                                                                                     | :heavy_minus_sign:                                                                                                                           | Optional target region override. When provided, the impersonated config<br/>uses this region instead of inheriting the caller's region.      |