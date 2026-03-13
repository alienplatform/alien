# DeploymentConnectionInfoResources

## Example Usage

```typescript
import { DeploymentConnectionInfoResources } from "@alienplatform/platform-api/models";

let value: DeploymentConnectionInfoResources = {
  type: "<value>",
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `type`                                                                                                                                                     | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior. |
| `publicUrl`                                                                                                                                                | *string*                                                                                                                                                   | :heavy_minus_sign:                                                                                                                                         | Public URL if resource has public ingress                                                                                                                  |