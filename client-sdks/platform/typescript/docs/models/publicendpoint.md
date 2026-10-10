# PublicEndpoint

## Example Usage

```typescript
import { PublicEndpoint } from "@alienplatform/platform-api/models";

let value: PublicEndpoint = {
  resourceId: "<id>",
  resourceType: "<value>",
  endpointName: "<value>",
  hostLabel: "<value>",
  wildcardSubdomains: false,
};
```

## Fields

| Field                                                                     | Type                                                                      | Required                                                                  | Description                                                               |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `resourceId`                                                              | *string*                                                                  | :heavy_check_mark:                                                        | N/A                                                                       |
| `resourceType`                                                            | *string*                                                                  | :heavy_check_mark:                                                        | Type of the resource that declares the endpoint, e.g. container or worker |
| `endpointName`                                                            | *string*                                                                  | :heavy_check_mark:                                                        | N/A                                                                       |
| `hostLabel`                                                               | *string*                                                                  | :heavy_check_mark:                                                        | N/A                                                                       |
| `wildcardSubdomains`                                                      | *boolean*                                                                 | :heavy_check_mark:                                                        | N/A                                                                       |