# ExternalBinding

A binding to an existing resource of a type an extension defines, such as a DNS zone the
customer already has. Its fields are the resource's outputs, as the type's descriptor declares
them in `schemas.outputs`.

## Example Usage

```typescript
import { ExternalBinding } from "@alienplatform/platform-api/models";

let value: ExternalBinding = {
  service: "<value>",
  type: "<value>",
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `service`                                                             | *string*                                                              | :heavy_check_mark:                                                    | The service the existing resource lives in, such as `route53`.        |
| `type`                                                                | *string*                                                              | :heavy_check_mark:                                                    | The type of a resource an extension defines, such as `acme/dns-zone`. |
| `additionalProperties`                                                | Record<string, *any*>                                                 | :heavy_minus_sign:                                                    | N/A                                                                   |