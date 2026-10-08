# PackagesConfigPatchLogCollector

## Example Usage

```typescript
import { PackagesConfigPatchLogCollector } from "@alienplatform/platform-api/models";

let value: PackagesConfigPatchLogCollector = {};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `enabled`                                                                  | *boolean*                                                                  | :heavy_minus_sign:                                                         | Whether logs are collected by default; installers can override this value. |
| `mode`                                                                     | [models.PackagesConfigPatchMode](../models/packagesconfigpatchmode.md)     | :heavy_minus_sign:                                                         | Kubernetes log collection mechanism.                                       |