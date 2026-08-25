use url::form_urlencoded;

pub struct PathBuilder {
    path: String,
    query: form_urlencoded::Serializer<'static, String>,
}

impl PathBuilder {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            query: form_urlencoded::Serializer::new(String::new()),
        }
    }

    #[must_use]
    pub fn query_opt(mut self, key: &str, value: Option<&str>) -> Self {
        if let Some(value) = value {
            self.query.append_pair(key, value);
        }
        self
    }

    pub fn build(mut self) -> String {
        let query = self.query.finish().clone();
        if query.is_empty() {
            self.path
        } else {
            format!("{}?{query}", self.path)
        }
    }
}
