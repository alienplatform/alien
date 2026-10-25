# CommandVariants

## Example Usage

```typescript
import { CommandVariants } from "@alienplatform/platform-api/models/operations";

let value: CommandVariants = {
  linux: {
    install: "<value>",
    command: "<value>",
  },
  mac: {
    install: "<value>",
    command: "<value>",
  },
  windows: {
    install: "<value>",
    command: "<value>",
  },
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `linux`                                                  | [operations.Linux](../../models/operations/linux.md)     | :heavy_check_mark:                                       | N/A                                                      |
| `mac`                                                    | [operations.Mac](../../models/operations/mac.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `windows`                                                | [operations.Windows](../../models/operations/windows.md) | :heavy_check_mark:                                       | N/A                                                      |