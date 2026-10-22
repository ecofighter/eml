-- `'x'u8` の値は文字のコードポイントで、255 を超えると E0007 になる。
too_large : Byte
too_large = 'あ'u8
