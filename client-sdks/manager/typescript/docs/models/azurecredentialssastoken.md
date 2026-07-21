# AzureCredentialsSasToken

A short-lived Azure Storage shared access signature.

Query parameter values are kept decoded. Azure clients must encode them
when attaching them to a request URL.

## Example Usage

```typescript
import { AzureCredentialsSasToken } from "@alienplatform/manager-api/models";

let value: AzureCredentialsSasToken = {
  queryParameters: {},
  type: "sasToken",
};
```

## Fields

| Field                                                           | Type                                                            | Required                                                        | Description                                                     |
| --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `queryParameters`                                               | Record<string, *string*>                                        | :heavy_check_mark:                                              | Exact SAS query parameters, including the signature and expiry. |
| `type`                                                          | *"sasToken"*                                                    | :heavy_check_mark:                                              | N/A                                                             |