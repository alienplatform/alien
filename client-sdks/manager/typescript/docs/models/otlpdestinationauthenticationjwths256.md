# OtlpDestinationAuthenticationJwtHs256

## Example Usage

```typescript
import { OtlpDestinationAuthenticationJwtHs256 } from "@alienplatform/manager-api/models";

let value: OtlpDestinationAuthenticationJwtHs256 = {
  audience: "<value>",
  claims: {
    "key": "<value>",
  },
  issuer: "jcb",
  keyEnvironmentVariable: "<value>",
  type: "jwtHs256",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `audience`                                                                           | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `claims`                                                                             | Record<string, *any*>                                                                | :heavy_check_mark:                                                                   | Application-owned claims. Standard issuer/audience/time claims cannot be overridden. |
| `issuer`                                                                             | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `keyEnvironmentVariable`                                                             | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `type`                                                                               | *"jwtHs256"*                                                                         | :heavy_check_mark:                                                                   | N/A                                                                                  |
