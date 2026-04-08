module Usage where

import Example

sayHello :: IO ()
sayHello = putStrLn (greet "World")

useAnimal :: Animal -> String
useAnimal a = show a
