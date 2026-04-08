module FoldingTest where

import Data.List
import Data.Maybe
import Data.Map (Map)
import qualified Data.Set as Set

processItems :: [Int] -> [Int]
processItems items = result
  where
    filtered = filter (> 0) items
    result = map (* 2) filtered

doSomething :: IO ()
doSomething = do
  let x = 42
  print x
