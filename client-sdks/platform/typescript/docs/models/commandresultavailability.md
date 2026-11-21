# CommandResultAvailability

Whether a successful operation result is included or withheld. `protected` means withheld: pending explicit confirmation, or kept in a Private Alien Instance (see `resultKeptInInstance`).

## Example Usage

```typescript
import { CommandResultAvailability } from "@alienplatform/platform-api/models";

let value: CommandResultAvailability = "protected";
```

## Values

```typescript
"available" | "protected"
```