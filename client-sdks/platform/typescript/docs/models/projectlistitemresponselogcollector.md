# ProjectListItemResponseLogCollector

Default log collection mode in a generated Helm chart.

## Example Usage

```typescript
import { ProjectListItemResponseLogCollector } from "@alienplatform/platform-api/models";

let value: ProjectListItemResponseLogCollector = {
  enabled: false,
  mode: "podApi",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `enabled`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | Whether logs are collected by default; installers can override this value.     |
| `mode`                                                                         | [models.ProjectListItemResponseMode](../models/projectlistitemresponsemode.md) | :heavy_check_mark:                                                             | Kubernetes log collection mechanism.                                           |
