# HelmOutputs

Outputs from a Helm chart package build

## Example Usage

```typescript
import { HelmOutputs } from "@alienplatform/platform-api/models";

let value: HelmOutputs = {
  chart: "<value>",
  version: "<value>",
};
```

## Fields

| Field                                                                     | Type                                                                      | Required                                                                  | Description                                                               |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `chart`                                                                   | *string*                                                                  | :heavy_check_mark:                                                        | OCI chart reference (e.g., "oci://public.ecr.aws/acme/charts/project-id") |
| `managerUrl`                                                              | *string*                                                                  | :heavy_minus_sign:                                                        | Default management endpoint embedded in this chart, if available.         |
| `version`                                                                 | *string*                                                                  | :heavy_check_mark:                                                        | Chart version (e.g., "1.2.3")                                             |