# BaseResourceOutputs

Resource outputs that can hold output data for any resource type in the Alien system. All resource outputs share a common 'type' field with additional type-specific output properties.

## Example Usage

```typescript
import { BaseResourceOutputs } from "@alienplatform/manager-api/models";

let value: BaseResourceOutputs = {
  type: "function",
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                | Example                                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `type`                                                                                                                                                     | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior. | function                                                                                                                                                   |
| `additionalProperties`                                                                                                                                     | Record<string, *any*>                                                                                                                                      | :heavy_minus_sign:                                                                                                                                         | N/A                                                                                                                                                        |                                                                                                                                                            |