module HoverTest where

-- | A documented greeter.
-- | Produces personalized greetings.
greet :: String -> String -> String
greet prefix name = prefix ++ ", " ++ name ++ "!"

-- | Count the items in a list.
--
-- Example:
--
-- > length [1,2,3]
-- > -- returns 3
countItems :: [a] -> Int
countItems = length

-- | Adds one. Uses @inline code@, /italic/ and __bold__ markup.
addOne :: Int -> Int
addOne x = x + 1

-- | A colour data type.
data Color = Red | Green | Blue

-- Undocumented function (no Haddock prefix).
undocumented :: Int -> Int
undocumented x = x
