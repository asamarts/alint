impl serde::Serialize for AdapterApiExtractRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.input.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.AdapterApiExtractRequest", len)?;
        if let Some(v) = self.input.as_ref() {
            struct_ser.serialize_field("input", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AdapterApiExtractRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "input",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Input,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "input" => Ok(GeneratedField::Input),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AdapterApiExtractRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.AdapterApiExtractRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AdapterApiExtractRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut input__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Input => {
                            if input__.is_some() {
                                return Err(serde::de::Error::duplicate_field("input"));
                            }
                            input__ = map_.next_value()?;
                        }
                    }
                }
                Ok(AdapterApiExtractRequest {
                    input: input__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.AdapterApiExtractRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AdapterApiExtractResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.result.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.AdapterApiExtractResponse", len)?;
        if let Some(v) = self.result.as_ref() {
            struct_ser.serialize_field("result", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AdapterApiExtractResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "result",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Result,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "result" => Ok(GeneratedField::Result),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AdapterApiExtractResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.AdapterApiExtractResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AdapterApiExtractResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut result__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Result => {
                            if result__.is_some() {
                                return Err(serde::de::Error::duplicate_field("result"));
                            }
                            result__ = map_.next_value()?;
                        }
                    }
                }
                Ok(AdapterApiExtractResponse {
                    result: result__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.AdapterApiExtractResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AdapterApiIdRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let len = 0;
        let struct_ser = serializer.serialize_struct("alint.plugin.v0.AdapterApiIdRequest", len)?;
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AdapterApiIdRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                            Err(serde::de::Error::unknown_field(value, FIELDS))
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AdapterApiIdRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.AdapterApiIdRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AdapterApiIdRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                while map_.next_key::<GeneratedField>()?.is_some() {
                    let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                }
                Ok(AdapterApiIdRequest {
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.AdapterApiIdRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AdapterApiIdResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.result.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.AdapterApiIdResponse", len)?;
        if let Some(v) = self.result.as_ref() {
            struct_ser.serialize_field("result", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AdapterApiIdResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "result",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Result,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "result" => Ok(GeneratedField::Result),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AdapterApiIdResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.AdapterApiIdResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AdapterApiIdResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut result__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Result => {
                            if result__.is_some() {
                                return Err(serde::de::Error::duplicate_field("result"));
                            }
                            result__ = map_.next_value()?;
                        }
                    }
                }
                Ok(AdapterApiIdResponse {
                    result: result__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.AdapterApiIdResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AdapterApiMethod {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "ADAPTER_API_METHOD_UNSPECIFIED",
            Self::Id => "ADAPTER_API_METHOD_ID",
            Self::RedactsByDefault => "ADAPTER_API_METHOD_REDACTS_BY_DEFAULT",
            Self::Extract => "ADAPTER_API_METHOD_EXTRACT",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for AdapterApiMethod {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ADAPTER_API_METHOD_UNSPECIFIED",
            "ADAPTER_API_METHOD_ID",
            "ADAPTER_API_METHOD_REDACTS_BY_DEFAULT",
            "ADAPTER_API_METHOD_EXTRACT",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = AdapterApiMethod;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "expected one of: {:?}", &FIELDS)
            }

            fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Signed(v), &self)
                    })
            }

            fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Unsigned(v), &self)
                    })
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "ADAPTER_API_METHOD_UNSPECIFIED" => Ok(AdapterApiMethod::Unspecified),
                    "ADAPTER_API_METHOD_ID" => Ok(AdapterApiMethod::Id),
                    "ADAPTER_API_METHOD_REDACTS_BY_DEFAULT" => Ok(AdapterApiMethod::RedactsByDefault),
                    "ADAPTER_API_METHOD_EXTRACT" => Ok(AdapterApiMethod::Extract),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for AdapterApiRedactsByDefaultRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let len = 0;
        let struct_ser = serializer.serialize_struct("alint.plugin.v0.AdapterApiRedactsByDefaultRequest", len)?;
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AdapterApiRedactsByDefaultRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                            Err(serde::de::Error::unknown_field(value, FIELDS))
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AdapterApiRedactsByDefaultRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.AdapterApiRedactsByDefaultRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AdapterApiRedactsByDefaultRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                while map_.next_key::<GeneratedField>()?.is_some() {
                    let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                }
                Ok(AdapterApiRedactsByDefaultRequest {
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.AdapterApiRedactsByDefaultRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AdapterApiRedactsByDefaultResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.result.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.AdapterApiRedactsByDefaultResponse", len)?;
        if let Some(v) = self.result.as_ref() {
            struct_ser.serialize_field("result", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AdapterApiRedactsByDefaultResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "result",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Result,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "result" => Ok(GeneratedField::Result),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AdapterApiRedactsByDefaultResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.AdapterApiRedactsByDefaultResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AdapterApiRedactsByDefaultResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut result__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Result => {
                            if result__.is_some() {
                                return Err(serde::de::Error::duplicate_field("result"));
                            }
                            result__ = map_.next_value()?;
                        }
                    }
                }
                Ok(AdapterApiRedactsByDefaultResponse {
                    result: result__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.AdapterApiRedactsByDefaultResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AnalyzerApiAnalyzeRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.input.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.AnalyzerApiAnalyzeRequest", len)?;
        if let Some(v) = self.input.as_ref() {
            struct_ser.serialize_field("input", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AnalyzerApiAnalyzeRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "input",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Input,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "input" => Ok(GeneratedField::Input),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AnalyzerApiAnalyzeRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.AnalyzerApiAnalyzeRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AnalyzerApiAnalyzeRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut input__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Input => {
                            if input__.is_some() {
                                return Err(serde::de::Error::duplicate_field("input"));
                            }
                            input__ = map_.next_value()?;
                        }
                    }
                }
                Ok(AnalyzerApiAnalyzeRequest {
                    input: input__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.AnalyzerApiAnalyzeRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AnalyzerApiAnalyzeResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.result.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.AnalyzerApiAnalyzeResponse", len)?;
        if let Some(v) = self.result.as_ref() {
            struct_ser.serialize_field("result", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AnalyzerApiAnalyzeResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "result",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Result,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "result" => Ok(GeneratedField::Result),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AnalyzerApiAnalyzeResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.AnalyzerApiAnalyzeResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AnalyzerApiAnalyzeResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut result__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Result => {
                            if result__.is_some() {
                                return Err(serde::de::Error::duplicate_field("result"));
                            }
                            result__ = map_.next_value()?;
                        }
                    }
                }
                Ok(AnalyzerApiAnalyzeResponse {
                    result: result__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.AnalyzerApiAnalyzeResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AnalyzerApiIdRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let len = 0;
        let struct_ser = serializer.serialize_struct("alint.plugin.v0.AnalyzerApiIdRequest", len)?;
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AnalyzerApiIdRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                            Err(serde::de::Error::unknown_field(value, FIELDS))
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AnalyzerApiIdRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.AnalyzerApiIdRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AnalyzerApiIdRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                while map_.next_key::<GeneratedField>()?.is_some() {
                    let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                }
                Ok(AnalyzerApiIdRequest {
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.AnalyzerApiIdRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AnalyzerApiIdResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.result.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.AnalyzerApiIdResponse", len)?;
        if let Some(v) = self.result.as_ref() {
            struct_ser.serialize_field("result", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AnalyzerApiIdResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "result",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Result,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "result" => Ok(GeneratedField::Result),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AnalyzerApiIdResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.AnalyzerApiIdResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AnalyzerApiIdResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut result__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Result => {
                            if result__.is_some() {
                                return Err(serde::de::Error::duplicate_field("result"));
                            }
                            result__ = map_.next_value()?;
                        }
                    }
                }
                Ok(AnalyzerApiIdResponse {
                    result: result__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.AnalyzerApiIdResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AnalyzerApiMethod {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "ANALYZER_API_METHOD_UNSPECIFIED",
            Self::Id => "ANALYZER_API_METHOD_ID",
            Self::Analyze => "ANALYZER_API_METHOD_ANALYZE",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for AnalyzerApiMethod {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ANALYZER_API_METHOD_UNSPECIFIED",
            "ANALYZER_API_METHOD_ID",
            "ANALYZER_API_METHOD_ANALYZE",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = AnalyzerApiMethod;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "expected one of: {:?}", &FIELDS)
            }

            fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Signed(v), &self)
                    })
            }

            fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Unsigned(v), &self)
                    })
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "ANALYZER_API_METHOD_UNSPECIFIED" => Ok(AnalyzerApiMethod::Unspecified),
                    "ANALYZER_API_METHOD_ID" => Ok(AnalyzerApiMethod::Id),
                    "ANALYZER_API_METHOD_ANALYZE" => Ok(AnalyzerApiMethod::Analyze),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for Empty {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let len = 0;
        let struct_ser = serializer.serialize_struct("alint.plugin.v0.Empty", len)?;
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Empty {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                            Err(serde::de::Error::unknown_field(value, FIELDS))
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Empty;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.Empty")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Empty, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                while map_.next_key::<GeneratedField>()?.is_some() {
                    let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                }
                Ok(Empty {
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.Empty", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesAdapterError {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.witforge_value.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesAdapterError", len)?;
        if let Some(v) = self.witforge_value.as_ref() {
            match v {
                types_adapter_error::WitforgeValue::Parse(v) => {
                    struct_ser.serialize_field("parse", v)?;
                }
                types_adapter_error::WitforgeValue::Limit(v) => {
                    struct_ser.serialize_field("limit", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesAdapterError {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "parse",
            "limit",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Parse,
            Limit,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "parse" => Ok(GeneratedField::Parse),
                            "limit" => Ok(GeneratedField::Limit),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesAdapterError;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesAdapterError")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesAdapterError, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut witforge_value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Parse => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("parse"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_adapter_error::WitforgeValue::Parse);
                        }
                        GeneratedField::Limit => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("limit"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_adapter_error::WitforgeValue::Limit)
;
                        }
                    }
                }
                Ok(TypesAdapterError {
                    witforge_value: witforge_value__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesAdapterError", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesAdapterInput {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.resource.is_some() {
            len += 1;
        }
        if self.bytes.is_some() {
            len += 1;
        }
        if self.max_rows.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesAdapterInput", len)?;
        if let Some(v) = self.resource.as_ref() {
            struct_ser.serialize_field("resource", v)?;
        }
        if let Some(v) = self.bytes.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("bytes", pbjson::private::base64::encode(&v).as_str())?;
        }
        if let Some(v) = self.max_rows.as_ref() {
            struct_ser.serialize_field("maxRows", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesAdapterInput {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "resource",
            "bytes",
            "max_rows",
            "maxRows",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Resource,
            Bytes,
            MaxRows,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "resource" => Ok(GeneratedField::Resource),
                            "bytes" => Ok(GeneratedField::Bytes),
                            "maxRows" | "max_rows" => Ok(GeneratedField::MaxRows),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesAdapterInput;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesAdapterInput")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesAdapterInput, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut resource__ = None;
                let mut bytes__ = None;
                let mut max_rows__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Resource => {
                            if resource__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resource"));
                            }
                            resource__ = map_.next_value()?;
                        }
                        GeneratedField::Bytes => {
                            if bytes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("bytes"));
                            }
                            bytes__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::BytesDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::MaxRows => {
                            if max_rows__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxRows"));
                            }
                            max_rows__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                    }
                }
                Ok(TypesAdapterInput {
                    resource: resource__,
                    bytes: bytes__,
                    max_rows: max_rows__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesAdapterInput", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesAdapterOutput {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.rows.is_empty() {
            len += 1;
        }
        if !self.non_strings.is_empty() {
            len += 1;
        }
        if !self.notes.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesAdapterOutput", len)?;
        if !self.rows.is_empty() {
            struct_ser.serialize_field("rows", &self.rows)?;
        }
        if !self.non_strings.is_empty() {
            struct_ser.serialize_field("nonStrings", &self.non_strings)?;
        }
        if !self.notes.is_empty() {
            struct_ser.serialize_field("notes", &self.notes)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesAdapterOutput {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "rows",
            "non_strings",
            "nonStrings",
            "notes",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Rows,
            NonStrings,
            Notes,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "rows" => Ok(GeneratedField::Rows),
                            "nonStrings" | "non_strings" => Ok(GeneratedField::NonStrings),
                            "notes" => Ok(GeneratedField::Notes),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesAdapterOutput;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesAdapterOutput")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesAdapterOutput, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut rows__ = None;
                let mut non_strings__ = None;
                let mut notes__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Rows => {
                            if rows__.is_some() {
                                return Err(serde::de::Error::duplicate_field("rows"));
                            }
                            rows__ = Some(map_.next_value()?);
                        }
                        GeneratedField::NonStrings => {
                            if non_strings__.is_some() {
                                return Err(serde::de::Error::duplicate_field("nonStrings"));
                            }
                            non_strings__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Notes => {
                            if notes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("notes"));
                            }
                            notes__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(TypesAdapterOutput {
                    rows: rows__.unwrap_or_default(),
                    non_strings: non_strings__.unwrap_or_default(),
                    notes: notes__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesAdapterOutput", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesAnalyzerError {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.witforge_value.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesAnalyzerError", len)?;
        if let Some(v) = self.witforge_value.as_ref() {
            match v {
                types_analyzer_error::WitforgeValue::InvalidInput(v) => {
                    struct_ser.serialize_field("invalidInput", v)?;
                }
                types_analyzer_error::WitforgeValue::Limit(v) => {
                    struct_ser.serialize_field("limit", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesAnalyzerError {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "invalid_input",
            "invalidInput",
            "limit",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            InvalidInput,
            Limit,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "invalidInput" | "invalid_input" => Ok(GeneratedField::InvalidInput),
                            "limit" => Ok(GeneratedField::Limit),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesAnalyzerError;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesAnalyzerError")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesAnalyzerError, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut witforge_value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::InvalidInput => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("invalidInput"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_analyzer_error::WitforgeValue::InvalidInput);
                        }
                        GeneratedField::Limit => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("limit"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_analyzer_error::WitforgeValue::Limit)
;
                        }
                    }
                }
                Ok(TypesAnalyzerError {
                    witforge_value: witforge_value__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesAnalyzerError", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesAnalyzerInput {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.rule_id.is_some() {
            len += 1;
        }
        if !self.rows.is_empty() {
            len += 1;
        }
        if self.max_findings.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesAnalyzerInput", len)?;
        if let Some(v) = self.rule_id.as_ref() {
            struct_ser.serialize_field("ruleId", v)?;
        }
        if !self.rows.is_empty() {
            struct_ser.serialize_field("rows", &self.rows)?;
        }
        if let Some(v) = self.max_findings.as_ref() {
            struct_ser.serialize_field("maxFindings", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesAnalyzerInput {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "rule_id",
            "ruleId",
            "rows",
            "max_findings",
            "maxFindings",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            RuleId,
            Rows,
            MaxFindings,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "ruleId" | "rule_id" => Ok(GeneratedField::RuleId),
                            "rows" => Ok(GeneratedField::Rows),
                            "maxFindings" | "max_findings" => Ok(GeneratedField::MaxFindings),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesAnalyzerInput;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesAnalyzerInput")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesAnalyzerInput, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut rule_id__ = None;
                let mut rows__ = None;
                let mut max_findings__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::RuleId => {
                            if rule_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ruleId"));
                            }
                            rule_id__ = map_.next_value()?;
                        }
                        GeneratedField::Rows => {
                            if rows__.is_some() {
                                return Err(serde::de::Error::duplicate_field("rows"));
                            }
                            rows__ = Some(map_.next_value()?);
                        }
                        GeneratedField::MaxFindings => {
                            if max_findings__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxFindings"));
                            }
                            max_findings__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                    }
                }
                Ok(TypesAnalyzerInput {
                    rule_id: rule_id__,
                    rows: rows__.unwrap_or_default(),
                    max_findings: max_findings__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesAnalyzerInput", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesCapabilities {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.spawn_user_command.is_some() {
            len += 1;
        }
        if self.host_keyring.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesCapabilities", len)?;
        if let Some(v) = self.spawn_user_command.as_ref() {
            struct_ser.serialize_field("spawnUserCommand", v)?;
        }
        if let Some(v) = self.host_keyring.as_ref() {
            struct_ser.serialize_field("hostKeyring", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesCapabilities {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "spawn_user_command",
            "spawnUserCommand",
            "host_keyring",
            "hostKeyring",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            SpawnUserCommand,
            HostKeyring,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "spawnUserCommand" | "spawn_user_command" => Ok(GeneratedField::SpawnUserCommand),
                            "hostKeyring" | "host_keyring" => Ok(GeneratedField::HostKeyring),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesCapabilities;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesCapabilities")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesCapabilities, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut spawn_user_command__ = None;
                let mut host_keyring__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::SpawnUserCommand => {
                            if spawn_user_command__.is_some() {
                                return Err(serde::de::Error::duplicate_field("spawnUserCommand"));
                            }
                            spawn_user_command__ = map_.next_value()?;
                        }
                        GeneratedField::HostKeyring => {
                            if host_keyring__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hostKeyring"));
                            }
                            host_keyring__ = map_.next_value()?;
                        }
                    }
                }
                Ok(TypesCapabilities {
                    spawn_user_command: spawn_user_command__,
                    host_keyring: host_keyring__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesCapabilities", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesFinding {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.path.is_some() {
            len += 1;
        }
        if self.message.is_some() {
            len += 1;
        }
        if self.line.is_some() {
            len += 1;
        }
        if self.column.is_some() {
            len += 1;
        }
        if !self.related.is_empty() {
            len += 1;
        }
        if self.baseline_key.is_some() {
            len += 1;
        }
        if self.sensitive.is_some() {
            len += 1;
        }
        if self.analysis_failure.is_some() {
            len += 1;
        }
        if self.failure_kind.is_some() {
            len += 1;
        }
        if self.not_fixable.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesFinding", len)?;
        if let Some(v) = self.path.as_ref() {
            struct_ser.serialize_field("path", v)?;
        }
        if let Some(v) = self.message.as_ref() {
            struct_ser.serialize_field("message", v)?;
        }
        if let Some(v) = self.line.as_ref() {
            struct_ser.serialize_field("line", v)?;
        }
        if let Some(v) = self.column.as_ref() {
            struct_ser.serialize_field("column", v)?;
        }
        if !self.related.is_empty() {
            struct_ser.serialize_field("related", &self.related)?;
        }
        if let Some(v) = self.baseline_key.as_ref() {
            struct_ser.serialize_field("baselineKey", v)?;
        }
        if let Some(v) = self.sensitive.as_ref() {
            struct_ser.serialize_field("sensitive", v)?;
        }
        if let Some(v) = self.analysis_failure.as_ref() {
            struct_ser.serialize_field("analysisFailure", v)?;
        }
        if let Some(v) = self.failure_kind.as_ref() {
            struct_ser.serialize_field("failureKind", v)?;
        }
        if let Some(v) = self.not_fixable.as_ref() {
            struct_ser.serialize_field("notFixable", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesFinding {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "path",
            "message",
            "line",
            "column",
            "related",
            "baseline_key",
            "baselineKey",
            "sensitive",
            "analysis_failure",
            "analysisFailure",
            "failure_kind",
            "failureKind",
            "not_fixable",
            "notFixable",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Path,
            Message,
            Line,
            Column,
            Related,
            BaselineKey,
            Sensitive,
            AnalysisFailure,
            FailureKind,
            NotFixable,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "path" => Ok(GeneratedField::Path),
                            "message" => Ok(GeneratedField::Message),
                            "line" => Ok(GeneratedField::Line),
                            "column" => Ok(GeneratedField::Column),
                            "related" => Ok(GeneratedField::Related),
                            "baselineKey" | "baseline_key" => Ok(GeneratedField::BaselineKey),
                            "sensitive" => Ok(GeneratedField::Sensitive),
                            "analysisFailure" | "analysis_failure" => Ok(GeneratedField::AnalysisFailure),
                            "failureKind" | "failure_kind" => Ok(GeneratedField::FailureKind),
                            "notFixable" | "not_fixable" => Ok(GeneratedField::NotFixable),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesFinding;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesFinding")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesFinding, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut path__ = None;
                let mut message__ = None;
                let mut line__ = None;
                let mut column__ = None;
                let mut related__ = None;
                let mut baseline_key__ = None;
                let mut sensitive__ = None;
                let mut analysis_failure__ = None;
                let mut failure_kind__ = None;
                let mut not_fixable__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Path => {
                            if path__.is_some() {
                                return Err(serde::de::Error::duplicate_field("path"));
                            }
                            path__ = map_.next_value()?;
                        }
                        GeneratedField::Message => {
                            if message__.is_some() {
                                return Err(serde::de::Error::duplicate_field("message"));
                            }
                            message__ = map_.next_value()?;
                        }
                        GeneratedField::Line => {
                            if line__.is_some() {
                                return Err(serde::de::Error::duplicate_field("line"));
                            }
                            line__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::Column => {
                            if column__.is_some() {
                                return Err(serde::de::Error::duplicate_field("column"));
                            }
                            column__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::Related => {
                            if related__.is_some() {
                                return Err(serde::de::Error::duplicate_field("related"));
                            }
                            related__ = Some(map_.next_value()?);
                        }
                        GeneratedField::BaselineKey => {
                            if baseline_key__.is_some() {
                                return Err(serde::de::Error::duplicate_field("baselineKey"));
                            }
                            baseline_key__ = map_.next_value()?;
                        }
                        GeneratedField::Sensitive => {
                            if sensitive__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sensitive"));
                            }
                            sensitive__ = map_.next_value()?;
                        }
                        GeneratedField::AnalysisFailure => {
                            if analysis_failure__.is_some() {
                                return Err(serde::de::Error::duplicate_field("analysisFailure"));
                            }
                            analysis_failure__ = map_.next_value()?;
                        }
                        GeneratedField::FailureKind => {
                            if failure_kind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("failureKind"));
                            }
                            failure_kind__ = map_.next_value()?;
                        }
                        GeneratedField::NotFixable => {
                            if not_fixable__.is_some() {
                                return Err(serde::de::Error::duplicate_field("notFixable"));
                            }
                            not_fixable__ = map_.next_value()?;
                        }
                    }
                }
                Ok(TypesFinding {
                    path: path__,
                    message: message__,
                    line: line__,
                    column: column__,
                    related: related__.unwrap_or_default(),
                    baseline_key: baseline_key__,
                    sensitive: sensitive__,
                    analysis_failure: analysis_failure__,
                    failure_kind: failure_kind__,
                    not_fixable: not_fixable__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesFinding", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesFlatValue {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.nodes.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesFlatValue", len)?;
        if !self.nodes.is_empty() {
            struct_ser.serialize_field("nodes", &self.nodes)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesFlatValue {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "nodes",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Nodes,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "nodes" => Ok(GeneratedField::Nodes),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesFlatValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesFlatValue")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesFlatValue, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut nodes__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Nodes => {
                            if nodes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("nodes"));
                            }
                            nodes__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(TypesFlatValue {
                    nodes: nodes__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesFlatValue", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesLimitError {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.limit.is_some() {
            len += 1;
        }
        if self.reached.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesLimitError", len)?;
        if let Some(v) = self.limit.as_ref() {
            struct_ser.serialize_field("limit", v)?;
        }
        if let Some(v) = self.reached.as_ref() {
            struct_ser.serialize_field("reached", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesLimitError {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "limit",
            "reached",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Limit,
            Reached,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "limit" => Ok(GeneratedField::Limit),
                            "reached" => Ok(GeneratedField::Reached),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesLimitError;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesLimitError")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesLimitError, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut limit__ = None;
                let mut reached__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Limit => {
                            if limit__.is_some() {
                                return Err(serde::de::Error::duplicate_field("limit"));
                            }
                            limit__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::Reached => {
                            if reached__.is_some() {
                                return Err(serde::de::Error::duplicate_field("reached"));
                            }
                            reached__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                    }
                }
                Ok(TypesLimitError {
                    limit: limit__,
                    reached: reached__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesLimitError", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesMethod {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "TYPES_METHOD_UNSPECIFIED",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for TypesMethod {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "TYPES_METHOD_UNSPECIFIED",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = TypesMethod;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "expected one of: {:?}", &FIELDS)
            }

            fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Signed(v), &self)
                    })
            }

            fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Unsigned(v), &self)
                    })
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "TYPES_METHOD_UNSPECIFIED" => Ok(TypesMethod::Unspecified),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for TypesNode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.witforge_value.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesNode", len)?;
        if let Some(v) = self.witforge_value.as_ref() {
            match v {
                types_node::WitforgeValue::Null(v) => {
                    struct_ser.serialize_field("null", v)?;
                }
                types_node::WitforgeValue::Bool(v) => {
                    struct_ser.serialize_field("bool", v)?;
                }
                types_node::WitforgeValue::Number(v) => {
                    struct_ser.serialize_field("number", v)?;
                }
                types_node::WitforgeValue::String(v) => {
                    struct_ser.serialize_field("string", v)?;
                }
                types_node::WitforgeValue::Bytes(v) => {
                    #[allow(clippy::needless_borrow)]
                    #[allow(clippy::needless_borrows_for_generic_args)]
                    struct_ser.serialize_field("bytes", pbjson::private::base64::encode(&v).as_str())?;
                }
                types_node::WitforgeValue::Path(v) => {
                    struct_ser.serialize_field("path", v)?;
                }
                types_node::WitforgeValue::List(v) => {
                    struct_ser.serialize_field("list", v)?;
                }
                types_node::WitforgeValue::Record(v) => {
                    struct_ser.serialize_field("record", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesNode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "null",
            "bool",
            "number",
            "string",
            "bytes",
            "path",
            "list",
            "record",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Null,
            Bool,
            Number,
            String,
            Bytes,
            Path,
            List,
            Record,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "null" => Ok(GeneratedField::Null),
                            "bool" => Ok(GeneratedField::Bool),
                            "number" => Ok(GeneratedField::Number),
                            "string" => Ok(GeneratedField::String),
                            "bytes" => Ok(GeneratedField::Bytes),
                            "path" => Ok(GeneratedField::Path),
                            "list" => Ok(GeneratedField::List),
                            "record" => Ok(GeneratedField::Record),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesNode")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesNode, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut witforge_value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Null => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("null"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_node::WitforgeValue::Null)
;
                        }
                        GeneratedField::Bool => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("bool"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_node::WitforgeValue::Bool);
                        }
                        GeneratedField::Number => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("number"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_node::WitforgeValue::Number);
                        }
                        GeneratedField::String => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("string"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_node::WitforgeValue::String);
                        }
                        GeneratedField::Bytes => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("bytes"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<::pbjson::private::BytesDeserialize<_>>>()?.map(|x| types_node::WitforgeValue::Bytes(x.0));
                        }
                        GeneratedField::Path => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("path"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_node::WitforgeValue::Path);
                        }
                        GeneratedField::List => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("list"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_node::WitforgeValue::List)
;
                        }
                        GeneratedField::Record => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("record"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_node::WitforgeValue::Record)
;
                        }
                    }
                }
                Ok(TypesNode {
                    witforge_value: witforge_value__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesNode", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesPluginInfo {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.name.is_some() {
            len += 1;
        }
        if self.version.is_some() {
            len += 1;
        }
        if self.contract.is_some() {
            len += 1;
        }
        if self.capabilities.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesPluginInfo", len)?;
        if let Some(v) = self.name.as_ref() {
            struct_ser.serialize_field("name", v)?;
        }
        if let Some(v) = self.version.as_ref() {
            struct_ser.serialize_field("version", v)?;
        }
        if let Some(v) = self.contract.as_ref() {
            struct_ser.serialize_field("contract", v)?;
        }
        if let Some(v) = self.capabilities.as_ref() {
            struct_ser.serialize_field("capabilities", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesPluginInfo {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "version",
            "contract",
            "capabilities",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
            Version,
            Contract,
            Capabilities,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "name" => Ok(GeneratedField::Name),
                            "version" => Ok(GeneratedField::Version),
                            "contract" => Ok(GeneratedField::Contract),
                            "capabilities" => Ok(GeneratedField::Capabilities),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesPluginInfo;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesPluginInfo")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesPluginInfo, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut version__ = None;
                let mut contract__ = None;
                let mut capabilities__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = map_.next_value()?;
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = map_.next_value()?;
                        }
                        GeneratedField::Contract => {
                            if contract__.is_some() {
                                return Err(serde::de::Error::duplicate_field("contract"));
                            }
                            contract__ = map_.next_value()?;
                        }
                        GeneratedField::Capabilities => {
                            if capabilities__.is_some() {
                                return Err(serde::de::Error::duplicate_field("capabilities"));
                            }
                            capabilities__ = map_.next_value()?;
                        }
                    }
                }
                Ok(TypesPluginInfo {
                    name: name__,
                    version: version__,
                    contract: contract__,
                    capabilities: capabilities__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesPluginInfo", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesPointerSegment {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.witforge_value.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesPointerSegment", len)?;
        if let Some(v) = self.witforge_value.as_ref() {
            match v {
                types_pointer_segment::WitforgeValue::Key(v) => {
                    struct_ser.serialize_field("key", v)?;
                }
                types_pointer_segment::WitforgeValue::Index(v) => {
                    #[allow(clippy::needless_borrow)]
                    #[allow(clippy::needless_borrows_for_generic_args)]
                    struct_ser.serialize_field("index", ToString::to_string(&v).as_str())?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesPointerSegment {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "key",
            "index",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Key,
            Index,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "key" => Ok(GeneratedField::Key),
                            "index" => Ok(GeneratedField::Index),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesPointerSegment;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesPointerSegment")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesPointerSegment, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut witforge_value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Key => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("key"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_pointer_segment::WitforgeValue::Key);
                        }
                        GeneratedField::Index => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("index"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| types_pointer_segment::WitforgeValue::Index(x.0));
                        }
                    }
                }
                Ok(TypesPointerSegment {
                    witforge_value: witforge_value__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesPointerSegment", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesRecordField {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.name.is_some() {
            len += 1;
        }
        if self.child.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesRecordField", len)?;
        if let Some(v) = self.name.as_ref() {
            struct_ser.serialize_field("name", v)?;
        }
        if let Some(v) = self.child.as_ref() {
            struct_ser.serialize_field("child", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesRecordField {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "child",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
            Child,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "name" => Ok(GeneratedField::Name),
                            "child" => Ok(GeneratedField::Child),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesRecordField;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesRecordField")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesRecordField, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut child__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = map_.next_value()?;
                        }
                        GeneratedField::Child => {
                            if child__.is_some() {
                                return Err(serde::de::Error::duplicate_field("child"));
                            }
                            child__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                    }
                }
                Ok(TypesRecordField {
                    name: name__,
                    child: child__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesRecordField", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesRelatedLocation {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.path.is_some() {
            len += 1;
        }
        if self.label.is_some() {
            len += 1;
        }
        if self.line.is_some() {
            len += 1;
        }
        if self.column.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesRelatedLocation", len)?;
        if let Some(v) = self.path.as_ref() {
            struct_ser.serialize_field("path", v)?;
        }
        if let Some(v) = self.label.as_ref() {
            struct_ser.serialize_field("label", v)?;
        }
        if let Some(v) = self.line.as_ref() {
            struct_ser.serialize_field("line", v)?;
        }
        if let Some(v) = self.column.as_ref() {
            struct_ser.serialize_field("column", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesRelatedLocation {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "path",
            "label",
            "line",
            "column",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Path,
            Label,
            Line,
            Column,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "path" => Ok(GeneratedField::Path),
                            "label" => Ok(GeneratedField::Label),
                            "line" => Ok(GeneratedField::Line),
                            "column" => Ok(GeneratedField::Column),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesRelatedLocation;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesRelatedLocation")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesRelatedLocation, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut path__ = None;
                let mut label__ = None;
                let mut line__ = None;
                let mut column__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Path => {
                            if path__.is_some() {
                                return Err(serde::de::Error::duplicate_field("path"));
                            }
                            path__ = map_.next_value()?;
                        }
                        GeneratedField::Label => {
                            if label__.is_some() {
                                return Err(serde::de::Error::duplicate_field("label"));
                            }
                            label__ = map_.next_value()?;
                        }
                        GeneratedField::Line => {
                            if line__.is_some() {
                                return Err(serde::de::Error::duplicate_field("line"));
                            }
                            line__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::Column => {
                            if column__.is_some() {
                                return Err(serde::de::Error::duplicate_field("column"));
                            }
                            column__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                    }
                }
                Ok(TypesRelatedLocation {
                    path: path__,
                    label: label__,
                    line: line__,
                    column: column__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesRelatedLocation", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesRepoResource {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.path.is_some() {
            len += 1;
        }
        if self.indexed.is_some() {
            len += 1;
        }
        if self.index.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesRepoResource", len)?;
        if let Some(v) = self.path.as_ref() {
            struct_ser.serialize_field("path", v)?;
        }
        if let Some(v) = self.indexed.as_ref() {
            struct_ser.serialize_field("indexed", v)?;
        }
        if let Some(v) = self.index.as_ref() {
            struct_ser.serialize_field("index", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesRepoResource {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "path",
            "indexed",
            "index",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Path,
            Indexed,
            Index,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "path" => Ok(GeneratedField::Path),
                            "indexed" => Ok(GeneratedField::Indexed),
                            "index" => Ok(GeneratedField::Index),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesRepoResource;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesRepoResource")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesRepoResource, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut path__ = None;
                let mut indexed__ = None;
                let mut index__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Path => {
                            if path__.is_some() {
                                return Err(serde::de::Error::duplicate_field("path"));
                            }
                            path__ = map_.next_value()?;
                        }
                        GeneratedField::Indexed => {
                            if indexed__.is_some() {
                                return Err(serde::de::Error::duplicate_field("indexed"));
                            }
                            indexed__ = map_.next_value()?;
                        }
                        GeneratedField::Index => {
                            if index__.is_some() {
                                return Err(serde::de::Error::duplicate_field("index"));
                            }
                            index__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                    }
                }
                Ok(TypesRepoResource {
                    path: path__,
                    indexed: indexed__,
                    index: index__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesRepoResource", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesRow {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.resource.is_some() {
            len += 1;
        }
        if self.pointer.is_some() {
            len += 1;
        }
        if self.label.is_some() {
            len += 1;
        }
        if self.ordinal.is_some() {
            len += 1;
        }
        if self.start.is_some() {
            len += 1;
        }
        if self.end.is_some() {
            len += 1;
        }
        if self.state.is_some() {
            len += 1;
        }
        if self.original.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesRow", len)?;
        if let Some(v) = self.resource.as_ref() {
            struct_ser.serialize_field("resource", v)?;
        }
        if let Some(v) = self.pointer.as_ref() {
            struct_ser.serialize_field("pointer", v)?;
        }
        if let Some(v) = self.label.as_ref() {
            struct_ser.serialize_field("label", v)?;
        }
        if let Some(v) = self.ordinal.as_ref() {
            struct_ser.serialize_field("ordinal", v)?;
        }
        if let Some(v) = self.start.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("start", ToString::to_string(&v).as_str())?;
        }
        if let Some(v) = self.end.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("end", ToString::to_string(&v).as_str())?;
        }
        if let Some(v) = self.state.as_ref() {
            struct_ser.serialize_field("state", v)?;
        }
        if let Some(v) = self.original.as_ref() {
            struct_ser.serialize_field("original", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesRow {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "resource",
            "pointer",
            "label",
            "ordinal",
            "start",
            "end",
            "state",
            "original",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Resource,
            Pointer,
            Label,
            Ordinal,
            Start,
            End,
            State,
            Original,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "resource" => Ok(GeneratedField::Resource),
                            "pointer" => Ok(GeneratedField::Pointer),
                            "label" => Ok(GeneratedField::Label),
                            "ordinal" => Ok(GeneratedField::Ordinal),
                            "start" => Ok(GeneratedField::Start),
                            "end" => Ok(GeneratedField::End),
                            "state" => Ok(GeneratedField::State),
                            "original" => Ok(GeneratedField::Original),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesRow;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesRow")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesRow, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut resource__ = None;
                let mut pointer__ = None;
                let mut label__ = None;
                let mut ordinal__ = None;
                let mut start__ = None;
                let mut end__ = None;
                let mut state__ = None;
                let mut original__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Resource => {
                            if resource__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resource"));
                            }
                            resource__ = map_.next_value()?;
                        }
                        GeneratedField::Pointer => {
                            if pointer__.is_some() {
                                return Err(serde::de::Error::duplicate_field("pointer"));
                            }
                            pointer__ = map_.next_value()?;
                        }
                        GeneratedField::Label => {
                            if label__.is_some() {
                                return Err(serde::de::Error::duplicate_field("label"));
                            }
                            label__ = map_.next_value()?;
                        }
                        GeneratedField::Ordinal => {
                            if ordinal__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ordinal"));
                            }
                            ordinal__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::Start => {
                            if start__.is_some() {
                                return Err(serde::de::Error::duplicate_field("start"));
                            }
                            start__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::End => {
                            if end__.is_some() {
                                return Err(serde::de::Error::duplicate_field("end"));
                            }
                            end__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::State => {
                            if state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("state"));
                            }
                            state__ = map_.next_value()?;
                        }
                        GeneratedField::Original => {
                            if original__.is_some() {
                                return Err(serde::de::Error::duplicate_field("original"));
                            }
                            original__ = map_.next_value()?;
                        }
                    }
                }
                Ok(TypesRow {
                    resource: resource__,
                    pointer: pointer__,
                    label: label__,
                    ordinal: ordinal__,
                    start: start__,
                    end: end__,
                    state: state__,
                    original: original__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesRow", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TypesRowState {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.witforge_value.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.TypesRowState", len)?;
        if let Some(v) = self.witforge_value.as_ref() {
            match v {
                types_row_state::WitforgeValue::Value(v) => {
                    struct_ser.serialize_field("value", v)?;
                }
                types_row_state::WitforgeValue::Key(v) => {
                    struct_ser.serialize_field("key", v)?;
                }
                types_row_state::WitforgeValue::Missing(v) => {
                    struct_ser.serialize_field("missing", v)?;
                }
                types_row_state::WitforgeValue::Error(v) => {
                    struct_ser.serialize_field("error", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TypesRowState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "value",
            "key",
            "missing",
            "error",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Value,
            Key,
            Missing,
            Error,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "value" => Ok(GeneratedField::Value),
                            "key" => Ok(GeneratedField::Key),
                            "missing" => Ok(GeneratedField::Missing),
                            "error" => Ok(GeneratedField::Error),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TypesRowState;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.TypesRowState")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TypesRowState, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut witforge_value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Value => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("value"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_row_state::WitforgeValue::Value)
;
                        }
                        GeneratedField::Key => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("key"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_row_state::WitforgeValue::Key)
;
                        }
                        GeneratedField::Missing => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("missing"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_row_state::WitforgeValue::Missing)
;
                        }
                        GeneratedField::Error => {
                            if witforge_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("error"));
                            }
                            witforge_value__ = map_.next_value::<::std::option::Option<_>>()?.map(types_row_state::WitforgeValue::Error);
                        }
                    }
                }
                Ok(TypesRowState {
                    witforge_value: witforge_value__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.TypesRowState", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for WitforgeListOfTypesFinding {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.values.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.WitforgeListOfTypesFinding", len)?;
        if !self.values.is_empty() {
            struct_ser.serialize_field("values", &self.values)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for WitforgeListOfTypesFinding {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "values",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Values,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "values" => Ok(GeneratedField::Values),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = WitforgeListOfTypesFinding;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.WitforgeListOfTypesFinding")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<WitforgeListOfTypesFinding, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut values__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Values => {
                            if values__.is_some() {
                                return Err(serde::de::Error::duplicate_field("values"));
                            }
                            values__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(WitforgeListOfTypesFinding {
                    values: values__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.WitforgeListOfTypesFinding", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for WitforgeListOfTypesPointerSegment {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.values.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.WitforgeListOfTypesPointerSegment", len)?;
        if !self.values.is_empty() {
            struct_ser.serialize_field("values", &self.values)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for WitforgeListOfTypesPointerSegment {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "values",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Values,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "values" => Ok(GeneratedField::Values),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = WitforgeListOfTypesPointerSegment;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.WitforgeListOfTypesPointerSegment")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<WitforgeListOfTypesPointerSegment, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut values__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Values => {
                            if values__.is_some() {
                                return Err(serde::de::Error::duplicate_field("values"));
                            }
                            values__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(WitforgeListOfTypesPointerSegment {
                    values: values__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.WitforgeListOfTypesPointerSegment", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for WitforgeListOfTypesRecordField {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.values.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.WitforgeListOfTypesRecordField", len)?;
        if !self.values.is_empty() {
            struct_ser.serialize_field("values", &self.values)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for WitforgeListOfTypesRecordField {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "values",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Values,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "values" => Ok(GeneratedField::Values),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = WitforgeListOfTypesRecordField;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.WitforgeListOfTypesRecordField")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<WitforgeListOfTypesRecordField, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut values__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Values => {
                            if values__.is_some() {
                                return Err(serde::de::Error::duplicate_field("values"));
                            }
                            values__ = Some(map_.next_value()?);
                        }
                    }
                }
                Ok(WitforgeListOfTypesRecordField {
                    values: values__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.WitforgeListOfTypesRecordField", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for WitforgeListOfU32 {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.values.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.WitforgeListOfU32", len)?;
        if !self.values.is_empty() {
            struct_ser.serialize_field("values", &self.values)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for WitforgeListOfU32 {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "values",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Values,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "values" => Ok(GeneratedField::Values),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = WitforgeListOfU32;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.WitforgeListOfU32")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<WitforgeListOfU32, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut values__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Values => {
                            if values__.is_some() {
                                return Err(serde::de::Error::duplicate_field("values"));
                            }
                            values__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                    }
                }
                Ok(WitforgeListOfU32 {
                    values: values__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.WitforgeListOfU32", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for WitforgeResultOfListOfTypesFindingAndTypesAnalyzerError {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.value.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.WitforgeResultOfListOfTypesFindingAndTypesAnalyzerError", len)?;
        if let Some(v) = self.value.as_ref() {
            match v {
                witforge_result_of_list_of_types_finding_and_types_analyzer_error::Value::Ok(v) => {
                    struct_ser.serialize_field("ok", v)?;
                }
                witforge_result_of_list_of_types_finding_and_types_analyzer_error::Value::Err(v) => {
                    struct_ser.serialize_field("err", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for WitforgeResultOfListOfTypesFindingAndTypesAnalyzerError {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ok",
            "err",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Ok,
            Err,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "ok" => Ok(GeneratedField::Ok),
                            "err" => Ok(GeneratedField::Err),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = WitforgeResultOfListOfTypesFindingAndTypesAnalyzerError;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.WitforgeResultOfListOfTypesFindingAndTypesAnalyzerError")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<WitforgeResultOfListOfTypesFindingAndTypesAnalyzerError, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Ok => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ok"));
                            }
                            value__ = map_.next_value::<::std::option::Option<_>>()?.map(witforge_result_of_list_of_types_finding_and_types_analyzer_error::Value::Ok)
;
                        }
                        GeneratedField::Err => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("err"));
                            }
                            value__ = map_.next_value::<::std::option::Option<_>>()?.map(witforge_result_of_list_of_types_finding_and_types_analyzer_error::Value::Err)
;
                        }
                    }
                }
                Ok(WitforgeResultOfListOfTypesFindingAndTypesAnalyzerError {
                    value: value__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.WitforgeResultOfListOfTypesFindingAndTypesAnalyzerError", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for WitforgeResultOfTypesAdapterOutputAndTypesAdapterError {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.value.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("alint.plugin.v0.WitforgeResultOfTypesAdapterOutputAndTypesAdapterError", len)?;
        if let Some(v) = self.value.as_ref() {
            match v {
                witforge_result_of_types_adapter_output_and_types_adapter_error::Value::Ok(v) => {
                    struct_ser.serialize_field("ok", v)?;
                }
                witforge_result_of_types_adapter_output_and_types_adapter_error::Value::Err(v) => {
                    struct_ser.serialize_field("err", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for WitforgeResultOfTypesAdapterOutputAndTypesAdapterError {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ok",
            "err",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Ok,
            Err,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "ok" => Ok(GeneratedField::Ok),
                            "err" => Ok(GeneratedField::Err),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = WitforgeResultOfTypesAdapterOutputAndTypesAdapterError;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct alint.plugin.v0.WitforgeResultOfTypesAdapterOutputAndTypesAdapterError")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<WitforgeResultOfTypesAdapterOutputAndTypesAdapterError, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Ok => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ok"));
                            }
                            value__ = map_.next_value::<::std::option::Option<_>>()?.map(witforge_result_of_types_adapter_output_and_types_adapter_error::Value::Ok)
;
                        }
                        GeneratedField::Err => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("err"));
                            }
                            value__ = map_.next_value::<::std::option::Option<_>>()?.map(witforge_result_of_types_adapter_output_and_types_adapter_error::Value::Err)
;
                        }
                    }
                }
                Ok(WitforgeResultOfTypesAdapterOutputAndTypesAdapterError {
                    value: value__,
                })
            }
        }
        deserializer.deserialize_struct("alint.plugin.v0.WitforgeResultOfTypesAdapterOutputAndTypesAdapterError", FIELDS, GeneratedVisitor)
    }
}
