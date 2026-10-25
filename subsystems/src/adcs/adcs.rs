// Подключаем тип ошибки подсистемы СОС.
use super::error::AdcsError;

// Duration нужен для задания интервала времени.
use core::time::Duration;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Receiver;
use embassy_time::Instant;
// Vector3 — вектор из трёх чисел типа f32.
// Здесь он используется для магнитного поля и управляющих сигналов катушек.
use nalgebra::Vector3;

// Импортируем интерфейсы и типы из ядра программы (core).
use sat_core::adcs::{
    AdcSystem,     // Общий интерфейс подсистемы СОС.
    AdcsCommand,   // Команды, поступающие в СОС.
    AdcsEvent,     // События, которые СОС должна сообщать наружу.
    AdcsMode,      // Режимы работы СОС: например, Idle и Detumbling.
    BdotAlgorithm, // Алгоритм магнитной стабилизации B-dot.
    Coils,         // Интерфейс управления магнитными катушками.
    Mag,           // Интерфейс чтения магнитометра.
};

// Импортируем общий интерфейс задержки.
use sat_core::time::Delay;

type CommandRx = Receiver<'static, CriticalSectionRawMutex, AdcsCommand, 4>;

// Состояние системы ориентации и стабилизации.
// Хранит текущий режим работы и последние значения, используемые алгоритмом.
#[derive(Default)]
pub struct AdcsState {
    mode: AdcsMode,            // Текущий режим работы СОС.
    outer_b: Vector3<f32>,     // Последний измеренный вектор магнитного поля.
    coil_levels: Vector3<f32>, // Последние заданные уровни управления катушками.
}

// Реализация подсистемы СОС.
//
// M — конкретная реализация магнитометра, реализующая интерфейс Mag.
// A — конкретный драйвер катушек, реализующий интерфейс Coils.
// D — реализация задержки, соответствующая интерфейсу Delay.
// Благодаря обобщениям ядро управления не зависит от конкретных моделей устройств.
pub struct AdcSystemImpl<M: Mag, A: Coils, D: Delay> {
    mag: M,                           // Объект для чтения магнитного поля.
    coils: A,                         // Объект для управления катушками.
    delay: D,                         // Объект для организации временных задержек.
    state: AdcsState,                 // Текущее состояние СОС.
    bdot: BdotAlgorithm,              // Экземпляр алгоритма B-dot.
    pending_cmd: Option<AdcsCommand>, // Команда, ожидающая обработки; None означает её отсутствие.
    dt: Option<u64>,
    prev_measurement_time: Option<Instant>,
    rx: CommandRx,
}

// Реализуем методы для работы подсистемы СОС.
impl<M: Mag, A: Coils, D: Delay> AdcSystemImpl<M, A, D> {
    // Создаёт экземпляр СОС и принимает необходимые аппаратные компоненты.
    pub fn new(mag: M, coils: A, delay: D, rx: CommandRx) -> Self {
        Self {
            mag,                           // Сохраняем переданный магнитометр.
            coils,                         // Сохраняем драйвер катушек.
            delay,                         // Сохраняем механизм задержки.
            state: AdcsState::default(),   // Инициализируем состояние значениями по умолчанию.
            bdot: BdotAlgorithm::new(1.0), // Создаём алгоритм B-dot. 1.0 - нужно рассчитать коэффициент
            pending_cmd: None,             // Пока нет ожидающих команд.
            dt: None,
            prev_measurement_time: None,
            rx,
        }
    }

    // Метод асинхронный, поэтому операции ввода-вывода могут ожидать завершения
    // без обязательной блокировки всего асинхронного исполнителя.
    pub async fn run(&mut self) -> Result<(), AdcsError> {
        loop {
            self.read_sensors().await;
            self.apply_control().await?;
            self.handle_commands().await;
            self.delay.delay(Duration::from_millis(200)).await;
        }
    }

    // Сохраняет команду для последующей обработки главным циклом.
    // Если вызвать этот метод несколько раз до обработки, новая команда
    // заменит предыдущую, поскольку хранится только один Option<AdcsCommand>.
    pub fn send_command(&mut self, cmd: AdcsCommand) {
        self.pending_cmd = Some(cmd);
    }

    // Читает магнитное поле и сохраняет результат в состоянии СОС.
    async fn read_sensors(&mut self) {
        let b = self.mag.read().await;
        let now = Instant::now();
        self.dt = self
            .prev_measurement_time
            .map(|prev| (now - prev).as_micros());
        self.prev_measurement_time = Some(now);
        self.state.outer_b = b;
    }

    // Рассчитывает и применяет управление в зависимости от режима СОС.
    async fn apply_control(&mut self) -> Result<(), AdcsError> {
        match self.state.mode {
            // В режиме ожидания задаём нулевые уровни управления.
            AdcsMode::Idle => {
                self.state.coil_levels = Vector3::zeros();
            }

            // Режим гашения вращения спутника.
            AdcsMode::Detumbling => {
                // Проверяем, есть ли интервал между измерениями.
                let u = match self.dt {
                    Some(micros) if micros > 0 => {
                        let dt = Duration::from_micros(micros);

                        // B-dot рассчитывает управляющий вектор
                        // по магнитному полю и интервалу времени.
                        self.bdot.calc_control(self.state.outer_b, dt)
                    }
                    // Для первого измерения производную не вычисляем.
                    _ => Vector3::zeros(),
                };

                // Сохраняем рассчитанные уровни управления.
                self.state.coil_levels = u;
            }
        }

        self.coils
            .apply_pwm(self.state.coil_levels)
            .await
            .map_err(|_| AdcsError::CoilsPwmError)?;

        Ok(())
    }

    // Проверяет наличие ожидающей команды и выполняет её.
    async fn handle_commands(&mut self) {
        // take() извлекает команду из Option и одновременно заменяет значение на None.
        if let Some(cmd) = self.pending_cmd.take() {
            // Определяем тип полученной команды.
            match cmd {
                // Команда смены режима работы СОС.
                AdcsCommand::SetMode(mode) => self.set_mode(mode),
            }

            // Эта строка избыточна: после take() pending_cmd уже равен None.
            self.pending_cmd = None;
        }
    }

    // Переключает режим работы СОС.
    fn set_mode(&mut self, mode: AdcsMode) {
        // Выполняем действия только при фактическом изменении режима.
        if self.state.mode != mode {
            // Сбрасываем сохранённое состояние перед переключением.
            self.reset();

            // Устанавливаем новый режим работы.
            self.state.mode = mode;
        }
    }

    // Сбрасывает состояние СОС до значений по умолчанию.
    fn reset(&mut self) {
        self.state = AdcsState::default();
        self.bdot.reset();
        self.dt = None;
        self.prev_measurement_time = None;
    }
}
