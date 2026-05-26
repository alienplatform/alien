# ConfigCloudformation

Configuration for CloudFormation packages

## Example Usage

```typescript
import { ConfigCloudformation } from "@alienplatform/platform-api/models";

let value: ConfigCloudformation = {
  type: "cloudformation",
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `displayName`                                                         | *string*                                                              | :heavy_minus_sign:                                                    | Human-friendly application name shown in generated install artifacts. |
| `type`                                                                | *"cloudformation"*                                                    | :heavy_check_mark:                                                    | N/A                                                                   |