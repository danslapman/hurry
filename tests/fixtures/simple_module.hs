module Example where

-- | A greeting function.
greet :: String -> String
greet name = "Hello, " ++ name ++ "!"

-- | Multi-equation Fibonacci — tests deduplication.
fib :: Int -> Int
fib 0 = 0
fib 1 = 1
fib n = fib (n-1) + fib (n-2)

-- | A data type for animals.
data Animal = Cat | Dog | Bird
  deriving (Show)

-- | A type class with a default implementation.
class Speak a where
  speak :: a -> String
  speakLoudly :: a -> String
  speakLoudly x = speak x ++ "!"

-- | A type synonym.
type Name = String

-- See also: https://example.com/haskell
