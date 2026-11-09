use nalgebra::Vector3;

pub enum AdcsEvent {
    MeasuredB(Vector3<f32>),       // Выполнил измерения магнитного поля
    MeasuredSun,                   // Выполнил измерения освещенности
    AppliedPwm(Vector3<f32>),      // Задал ШИМ
    MeasuredCurrent(Vector3<f32>), // Получение тока
}
