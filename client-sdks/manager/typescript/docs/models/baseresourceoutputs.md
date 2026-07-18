# BaseResourceOutputs

Resource outputs that can hold output data for any resource type in the Alien system. All resource outputs share a common 'type' field with additional type-specific output properties.

## Example Usage

```typescript
import { BaseResourceOutputs } from "@alienplatform/manager-api/models";

let value: BaseResourceOutputs = {
  type: "worker",
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                | Example                                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `type`                                                                                                                                                     | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior. | **Example 1:** worker<br/>**Example 2:** storage<br/>**Example 3:** queue<br/>**Example 4:** redis<br/>**Example 5:** postgres                             |
| `additionalProperties`                                                                                                                                     | Record<string, *any*>                                                                                                                                      | :heavy_minus_sign:                                                                                                                                         | N/A                                                                                                                                                        |                                                                                                                                                            |