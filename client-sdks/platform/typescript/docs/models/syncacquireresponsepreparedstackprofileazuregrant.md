# SyncAcquireResponsePreparedStackProfileAzureGrant

Grant permissions for a specific cloud platform

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackProfileAzureGrant } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePreparedStackProfileAzureGrant = {};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `actions`                      | *string*[]                     | :heavy_minus_sign:             | AWS IAM actions (only for AWS) |
| `dataActions`                  | *string*[]                     | :heavy_minus_sign:             | Azure actions (only for Azure) |
| `permissions`                  | *string*[]                     | :heavy_minus_sign:             | GCP permissions (only for GCP) |