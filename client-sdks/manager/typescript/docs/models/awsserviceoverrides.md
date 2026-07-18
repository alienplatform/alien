# AwsServiceOverrides

Service endpoint overrides for testing AWS services

## Example Usage

```typescript
import { AwsServiceOverrides } from "@alienplatform/manager-api/models";

let value: AwsServiceOverrides = {
  endpoints: {
    "key": "<value>",
  },
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `endpoints`                                                                                                        | Record<string, *string*>                                                                                           | :heavy_check_mark:                                                                                                 | Override endpoints for specific AWS services<br/>Key is the service name (e.g., "lambda", "s3"), value is the base URL |