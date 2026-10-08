# UpdateProjectPackagesConfigLogCollector

Default log collection mode in a generated Helm chart.

## Example Usage

```typescript
import { UpdateProjectPackagesConfigLogCollector } from "@alienplatform/platform-api/models";

let value: UpdateProjectPackagesConfigLogCollector = {
  enabled: true,
  mode: "podApi",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `enabled`                                                                              | *boolean*                                                                              | :heavy_check_mark:                                                                     | Whether logs are collected by default; installers can override this value.             |
| `mode`                                                                                 | [models.UpdateProjectPackagesConfigMode](../models/updateprojectpackagesconfigmode.md) | :heavy_check_mark:                                                                     | Kubernetes log collection mechanism.                                                   |
