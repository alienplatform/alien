# AzureServiceOverrides

Service endpoint overrides for testing Azure services

## Example Usage

```typescript
import { AzureServiceOverrides } from "@alienplatform/manager-api/models";

let value: AzureServiceOverrides = {
  endpoints: {
    "key": "<value>",
    "key1": "<value>",
  },
};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `endpoints`                                                                                                                                    | Record<string, *string*>                                                                                                                       | :heavy_check_mark:                                                                                                                             | Override endpoints for specific Azure services<br/>Key is the service name (e.g., "management", "storage", "containerApps"), value is the base URL |