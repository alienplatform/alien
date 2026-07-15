# GcpServiceOverrides

Service endpoint overrides for testing GCP services

## Example Usage

```typescript
import { GcpServiceOverrides } from "@alienplatform/manager-api/models";

let value: GcpServiceOverrides = {
  endpoints: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
};
```

## Fields

| Field                                                                                                                     | Type                                                                                                                      | Required                                                                                                                  | Description                                                                                                               |
| ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `endpoints`                                                                                                               | Record<string, *string*>                                                                                                  | :heavy_check_mark:                                                                                                        | Override endpoints for specific GCP services<br/>Key is the service name (e.g., "cloudrun", "storage"), value is the base URL |