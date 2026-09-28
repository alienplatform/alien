# ProjectLogCollector

Default log collection mode in a generated Helm chart.

## Example Usage

```typescript
import { ProjectLogCollector } from "@alienplatform/platform-api/models";

let value: ProjectLogCollector = {
  enabled: false,
  mode: "podApi",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `enabled`                                                                  | *boolean*                                                                  | :heavy_check_mark:                                                         | Whether logs are collected by default; installers can override this value. |
| `mode`                                                                     | [models.ProjectMode](../models/projectmode.md)                             | :heavy_check_mark:                                                         | Kubernetes log collection mechanism.                                       |