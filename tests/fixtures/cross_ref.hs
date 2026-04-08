module CrossRef where

-- Where-bound name resolution.
compute :: Int -> Int -> Int
compute x y = total
  where
    total = x + y

-- Let-bound name resolution.
withLet :: Int -> Int
withLet n =
  let double = n * 2
  in double + 1
