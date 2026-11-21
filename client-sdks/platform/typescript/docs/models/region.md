# Region

## Example Usage

```typescript
import { Region } from "@alienplatform/platform-api/models";

let value: Region = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `secretRef`                                            | [models.RegionSecretRef](../models/regionsecretref.md) | :heavy_check_mark:                                     | Reference to a Kubernetes Secret                       |