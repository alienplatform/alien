# PackagePermissions

Versioned Kubernetes API requirements declared by an enabled operation.

## Example Usage

```typescript
import { PackagePermissions } from "@alienplatform/platform-api/models";

let value: PackagePermissions = {
  rules: [],
  schemaVersion: 980113,
};
```

## Fields

| Field                                                   | Type                                                    | Required                                                | Description                                             |
| ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- |
| `rules`                                                 | [models.PackageRule](../models/packagerule.md)[]        | :heavy_check_mark:                                      | Explicit Kubernetes API requirements.                   |
| `schemaVersion`                                         | *number*                                                | :heavy_check_mark:                                      | Schema version of the validated permission declaration. |