-- 抽象型のフィールドは、このモジュールの中だけで射影と更新ができる
pub abstract data Config = | Config { name : String, size : Int }

pub config : String -> Config
config n = Config { name = n, size = 1 }

pub name_of : Config -> String
name_of c = c.name
