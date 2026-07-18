# ClientConfigLocal

## Example Usage

```typescript
import { ClientConfigLocal } from "@alienplatform/manager-api/models";

let value: ClientConfigLocal = {
  platform: "local",
  stateDirectory: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `platform`                                               | [models.PlatformLocal](../models/platformlocal.md)       | :heavy_check_mark:                                       | N/A                                                      |
| `stateDirectory`                                         | *string*                                                 | :heavy_check_mark:                                       | State directory for local resources and deployment state |