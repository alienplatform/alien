# ReportedTelemetry

One OTLP batch from the environment.

## Example Usage

```typescript
import { ReportedTelemetry } from "@alienplatform/manager-api/models";

let value: ReportedTelemetry = {
  data: "<value>",
  signal: "<value>",
};
```

## Fields

| Field                                                                                                                                                                          | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `data`                                                                                                                                                                         | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | OTLP protobuf, base64.                                                                                                                                                         |
| `id`                                                                                                                                                                           | *number*                                                                                                                                                                       | :heavy_minus_sign:                                                                                                                                                             | The site's sequence number for the batch. The manager keeps the<br/>highest one it has passed on and skips any it already has, so a<br/>report sent again doesn't duplicate telemetry. |
| `signal`                                                                                                                                                                       | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | `logs`, `metrics` or `traces`.                                                                                                                                                 |