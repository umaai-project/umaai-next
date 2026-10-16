//! 固定长度属性数组别名与逐元素工具
//!
//! 游戏数据与领域类型共用的基础类型，避免各处重复书写裸数组。
//! 逐元素运算与六元组拆分是与游戏无关的纯函数工具，随别名一并置核心层。

use crate::{UmaError, UmaResult};

/// 五维属性：速 / 耐 / 力 / 根 / 智
pub type Array5 = [i32; 5];

/// 五维 + 技能点：速 / 耐 / 力 / 根 / 智 / pt
pub type Array6 = [i32; 6];

/// 属性数组逐元素运算（任意 `[i32; N]` 通用）
pub trait AttributeArray {
    /// 逐元素加法（`+=`），返回自身引用以便链式调用
    fn add_eq(&mut self, other: &Self) -> &mut Self;

    /// 是否全零
    fn is_default(&self) -> bool;
}

impl<const N: usize> AttributeArray for [i32; N] {
    fn add_eq(&mut self, other: &Self) -> &mut Self {
        for (x, y) in self.iter_mut().zip(other) {
            *x += y;
        }
        self
    }

    fn is_default(&self) -> bool {
        self.iter().all(|x| *x == 0)
    }
}

/// 把六元组（五维 + PT）拆为（五维引用， PT 值）
///
/// 长度恒为 6，失败分支仅作防御；正常输入永不报错。
pub fn split_status(status_pt: &Array6) -> UmaResult<(&Array5, i32)> {
    let (five, _) = status_pt
        .split_first_chunk()
        .ok_or_else(|| UmaError::Internal("Array6 长度不足 5".to_string()))?;
    Ok((five, status_pt[5]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：逐元素加法与全零判定
    #[test]
    fn test_add_eq_and_is_default() {
        let mut a: Array5 = [1, 2, 3, 4, 5];
        assert!(a.add_eq(&[10, 20, 30, 40, 50]).iter().eq(&[11, 22, 33, 44, 55]));
        assert!(!a.is_default());

        let mut b: Array6 = [0, 0, 0, 0, 0, 7];
        assert!(!b.is_default());
        b.add_eq(&[0, 0, 0, 0, 0, -7]);
        assert!(b.is_default());
    }

    /// 层1：六元组拆分（五维引用 + PT）
    #[test]
    fn test_split_status() {
        let s: Array6 = [1, 2, 3, 4, 5, 99];
        let (five, pt) = split_status(&s).expect("恒定长度");
        assert_eq!(five, &[1, 2, 3, 4, 5]);
        assert_eq!(pt, 99);
    }
}
