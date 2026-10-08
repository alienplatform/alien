# PackageLogCollector

Default log collection mode in a generated Helm chart.

## Example Usage

```typescript
import { PackageLogCollector } from "@alienplatform/platform-api/models";

let value: PackageLogCollector = {
  enabled: true,
  mode: "podApi",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `enabled`                                                                  | *boolean*                                                                  | :heavy_check_mark:                                                         | Whether logs are collected by default; installers can override this value. |
| `mode`                                                                     | [models.PackageMode](../models/packagemode.md)                             | :heavy_check_mark:                                                         | Kubernetes log collection mechanism.                                       |
