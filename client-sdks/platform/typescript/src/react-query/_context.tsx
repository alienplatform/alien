
import React from "react";

import { AlienCore } from "../core.js";

const AlienContext = React.createContext<AlienCore | null>(null);

export function AlienProvider(props: { client: AlienCore, children: React.ReactNode }): React.ReactNode { 
  return (
    <AlienContext.Provider value={props.client}>
      {props.children}
    </AlienContext.Provider>
  );
}

export function useAlienContext(): AlienCore { 
  const value = React.useContext(AlienContext);
  if (value === null) {
    throw new Error("SDK not initialized. Create an instance of AlienCore and pass it to <AlienProvider />.");
  }
  return value;
}
