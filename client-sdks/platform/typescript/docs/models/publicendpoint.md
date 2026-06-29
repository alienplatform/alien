# PublicEndpoint

## Example Usage

```typescript
import { PublicEndpoint } from "@alienplatform/platform-api/models";

let value: PublicEndpoint = {
  resourceId: "<id>",
  endpointName: "<value>",
  hostLabel: "<value>",
  wildcardSubdomains: false,
};
```

## Fields

| Field                | Type                 | Required             | Description          |
| -------------------- | -------------------- | -------------------- | -------------------- |
| `resourceId`         | *string*             | :heavy_check_mark:   | N/A                  |
| `endpointName`       | *string*             | :heavy_check_mark:   | N/A                  |
| `hostLabel`          | *string*             | :heavy_check_mark:   | N/A                  |
| `wildcardSubdomains` | *boolean*            | :heavy_check_mark:   | N/A                  |