import { StackStateSchema } from "../stack.js"

// Utility to get stack state in an Alien integration test
export function getStackState() {
  return StackStateSchema.parse(JSON.parse(process.env.ALIEN_STACK_STATE!))
}
