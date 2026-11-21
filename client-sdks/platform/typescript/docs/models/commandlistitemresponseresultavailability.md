# CommandListItemResponseResultAvailability

Whether a successful operation result is included or withheld. `protected` means withheld: pending explicit confirmation, or kept in a Private Alien Instance (see `resultKeptInInstance`).

## Example Usage

```typescript
import { CommandListItemResponseResultAvailability } from "@alienplatform/platform-api/models";

let value: CommandListItemResponseResultAvailability = "protected";
```

## Values

```typescript
"available" | "protected"
```